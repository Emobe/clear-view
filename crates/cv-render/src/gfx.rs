use std::num::NonZeroIsize;

use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle,
};
use windows::Win32::Foundation::HWND;

/// shader.wgsl with the cleanEdge port (MIT, see clean_edge.wgsl) appended. WGSL has no `#include`.
const SHADER: &str = concat!(include_str!("shader.wgsl"), "\n", include_str!("clean_edge.wgsl"));

/// Size of the `Uniforms` struct in shader.wgsl.
const UNIFORM_SIZE: u64 = 48;

#[allow(dead_code)] // fields kept alive for GPU resource lifetime
pub struct WgpuState {
    pub device:     wgpu::Device,
    pub queue:      wgpu::Queue,
    surface:        wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    pipeline:       wgpu::RenderPipeline,
    frame_tex:      wgpu::Texture,
    frame_view:     wgpu::TextureView,
    sampler:        wgpu::Sampler,
    uniform_buf:    wgpu::Buffer,
    bgl:            wgpu::BindGroupLayout,
    bind_group:     wgpu::BindGroup,
    pub tex_w:      u32,
    pub tex_h:      u32,
}

impl WgpuState {
    pub fn new(hwnd: HWND, win_w: u32, win_h: u32, tex_w: u32, tex_h: u32) -> Self {
        pollster::block_on(Self::init(hwnd, win_w, win_h, tex_w, tex_h))
    }

    async fn init(hwnd: HWND, win_w: u32, win_h: u32, tex_w: u32, tex_h: u32) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            ..Default::default()
        });

        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: RawDisplayHandle::Windows(WindowsDisplayHandle::new()),
                raw_window_handle:  RawWindowHandle::Win32(
                    Win32WindowHandle::new(NonZeroIsize::new(hwnd.0 as isize).unwrap())
                ),
            })
        }
        .expect("create_surface_unsafe failed");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference:       wgpu::PowerPreference::HighPerformance,
                compatible_surface:     Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("No DX12 adapter found");

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label:              Some("cv-render"),
                    required_features:  wgpu::Features::empty(),
                    required_limits:    wgpu::Limits::default(),
                    memory_hints:       wgpu::MemoryHints::default(),
                },
                None,
            )
            .await
            .expect("request_device failed");

        // Prefer Bgra8Unorm — matches DXGI capture format exactly, no swizzle.
        let caps = surface.get_capabilities(&adapter);
        let surface_format = caps
            .formats
            .iter()
            .find(|&&f| f == wgpu::TextureFormat::Bgra8Unorm)
            .copied()
            .unwrap_or(caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage:                          wgpu::TextureUsages::RENDER_ATTACHMENT,
            format:                         surface_format,
            width:                          win_w.max(1),
            height:                         win_h.max(1),
            present_mode:                   wgpu::PresentMode::Fifo,
            alpha_mode:                     caps.alpha_modes[0],
            view_formats:                   vec![],
            desired_maximum_frame_latency:  2,
        };
        surface.configure(&device, &surface_config);

        let (pipeline, bgl) = Self::make_pipeline(&device, surface_format);
        let (frame_tex, frame_view) = Self::make_frame_texture(&device, tex_w, tex_h);
        let sampler = Self::make_sampler(&device);
        let uniform_buf = Self::make_uniform_buf(&device);
        let bind_group = Self::make_bind_group(&device, &bgl, &frame_view, &sampler, &uniform_buf);

        Self {
            device, queue, surface, surface_config,
            pipeline, frame_tex, frame_view, sampler,
            uniform_buf, bgl, bind_group,
            tex_w, tex_h,
        }
    }

    /// The magnify pipeline and its bind group layout, rendering into `format`.
    fn make_pipeline(device: &wgpu::Device, format: wgpu::TextureFormat) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:  Some("magnify"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label:   Some("magnify-bgl"),
            entries: &[
                // binding 0 — Uniforms (48 bytes: crop + modes + cursor + edge threshold + padding)
                wgpu::BindGroupLayoutEntry {
                    binding:    0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty:                 wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size:   wgpu::BufferSize::new(UNIFORM_SIZE),
                    },
                    count: None,
                },
                // binding 1 — frame texture
                wgpu::BindGroupLayoutEntry {
                    binding:    1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type:    wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled:   false,
                    },
                    count: None,
                },
                // binding 2 — bilinear sampler
                wgpu::BindGroupLayoutEntry {
                    binding:    2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty:         wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count:      None,
                },
            ],
        });

        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label:                Some("magnify-pl"),
            bind_group_layouts:   &[&bgl],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:  Some("magnify"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module:              &shader,
                entry_point:         "vs",
                buffers:             &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module:      &shader,
                entry_point: "fs",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend:      Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive:     wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample:   wgpu::MultisampleState::default(),
            multiview:     None,
            cache:         None,
        });

        (pipeline, bgl)
    }

    fn make_sampler(device: &wgpu::Device) -> wgpu::Sampler {
        device.create_sampler(&wgpu::SamplerDescriptor {
            label:           Some("magnify-sampler"),
            address_mode_u:  wgpu::AddressMode::ClampToEdge,
            address_mode_v:  wgpu::AddressMode::ClampToEdge,
            mag_filter:      wgpu::FilterMode::Linear,
            min_filter:      wgpu::FilterMode::Linear,
            ..Default::default()
        })
    }

    fn make_uniform_buf(device: &wgpu::Device) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label:              Some("uniforms"),
            size:               UNIFORM_SIZE,
            usage:              wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn make_frame_texture(device: &wgpu::Device, w: u32, h: u32) -> (wgpu::Texture, wgpu::TextureView) {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label:           Some("frame-tex"),
            size:            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count:    1,
            dimension:       wgpu::TextureDimension::D2,
            format:          wgpu::TextureFormat::Bgra8Unorm,
            usage:           wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats:    &[],
        });
        let view = tex.create_view(&Default::default());
        (tex, view)
    }

    fn make_bind_group(
        device:      &wgpu::Device,
        bgl:         &wgpu::BindGroupLayout,
        frame_view:  &wgpu::TextureView,
        sampler:     &wgpu::Sampler,
        uniform_buf: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label:   Some("magnify-bg"),
            layout:  bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(frame_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        })
    }

    /// Replace the frame texture with one of a new size.
    /// Called when switching to a monitor with different resolution.
    pub fn recreate_frame_texture(&mut self, w: u32, h: u32) {
        let (frame_tex, frame_view) = Self::make_frame_texture(&self.device, w, h);
        self.bind_group = Self::make_bind_group(
            &self.device, &self.bgl, &frame_view, &self.sampler, &self.uniform_buf,
        );
        self.frame_tex  = frame_tex;
        self.frame_view = frame_view;
        self.tex_w = w;
        self.tex_h = h;
    }

    /// Reconfigure the surface after a window resize.
    pub fn resize(&mut self, new_w: u32, new_h: u32) {
        let w = new_w.max(16);
        let h = new_h.max(16);
        self.surface_config.width  = w;
        self.surface_config.height = h;
        self.surface.configure(&self.device, &self.surface_config);
    }

    /// Upload a new BGRA8 top-down frame to the GPU texture.
    /// Silently skips if dimensions don't match the texture.
    pub fn upload_frame(&self, data: &[u8], w: u32, h: u32) {
        if w != self.tex_w || h != self.tex_h { return; }
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture:   &self.frame_tex,
                mip_level: 0,
                origin:    wgpu::Origin3d::ZERO,
                aspect:    wgpu::TextureAspect::All,
            },
            data,
            wgpu::ImageDataLayout {
                offset:         0,
                bytes_per_row:  Some(w * 4),
                rows_per_image: None,
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    /// Write all uniforms to the 48-byte buffer.
    /// crop:           [src_x, src_y, src_w, src_h] normalised to [0, 1].
    /// color_mode:     ColorFilter::as_u32()   — 0=None,1=Inverted,2=Greyscale,3=GreyscaleInverted.
    /// interp_mode:    Interpolation::as_u32() — 0=Bilinear, 1=Bicubic, 2=Sharp, 3=CleanEdge.
    /// cursor_x/y:     software cursor position in output window pixels.
    /// edge_threshold: cleanEdge colour similarity threshold (0–1).
    pub fn write_uniforms(
        &self,
        crop: [f32; 4],
        color_mode: u32,
        interp_mode: u32,
        cursor_x: u32,
        cursor_y: u32,
        edge_threshold: f32,
    ) {
        let bytes = uniform_bytes(crop, color_mode, interp_mode, cursor_x, cursor_y, edge_threshold);
        self.queue.write_buffer(&self.uniform_buf, 0, &bytes);
    }

    /// Execute the render pass and present.
    /// Returns false on surface error (caller should call resize to recover).
    pub fn render(&self) -> bool {
        let output = match self.surface.get_current_texture() {
            Ok(o)  => o,
            Err(e) => {
                eprintln!("[render] surface error: {e:?}");
                return false;
            }
        };

        let view = output.texture.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        encode_pass(&mut enc, &view, &self.pipeline, &self.bind_group);
        self.queue.submit([enc.finish()]);
        output.present();
        true
    }
}

/// The uniform buffer contents; layout matches `Uniforms` in shader.wgsl.
fn uniform_bytes(
    crop: [f32; 4],
    color_mode: u32,
    interp_mode: u32,
    cursor_x: u32,
    cursor_y: u32,
    edge_threshold: f32,
) -> [u8; UNIFORM_SIZE as usize] {
    let mut bytes = [0u8; UNIFORM_SIZE as usize];
    for (i, &f) in crop.iter().enumerate() {
        bytes[i * 4..(i + 1) * 4].copy_from_slice(&f.to_ne_bytes());
    }
    bytes[16..20].copy_from_slice(&color_mode.to_ne_bytes());
    bytes[20..24].copy_from_slice(&interp_mode.to_ne_bytes());
    bytes[24..28].copy_from_slice(&cursor_x.to_ne_bytes());
    bytes[28..32].copy_from_slice(&cursor_y.to_ne_bytes());
    bytes[32..36].copy_from_slice(&edge_threshold.to_ne_bytes());
    // 36..48: padding
    bytes
}

/// One fullscreen-quad pass into `view`.
fn encode_pass(
    enc:        &mut wgpu::CommandEncoder,
    view:       &wgpu::TextureView,
    pipeline:   &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
) {
    let mut rpass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("magnify-pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load:  wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes:         None,
        occlusion_query_set:      None,
    });
    rpass.set_pipeline(pipeline);
    rpass.set_bind_group(0, bind_group, &[]);
    rpass.draw(0..6, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use wgpu::naga;

    /// wgpu only validates the shader when the pipeline is created at runtime; this catches errors in `cargo test`.
    #[test]
    fn shader_parses_and_validates() {
        let module = naga::front::wgsl::parse_str(SHADER)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(SHADER)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(SHADER)));
    }

    #[test]
    fn uniform_size_matches_the_shader_struct() {
        let module = naga::front::wgsl::parse_str(SHADER).unwrap();
        let ty = module.types.iter()
            .find(|(_, t)| t.name.as_deref() == Some("Uniforms"))
            .expect("Uniforms struct in shader.wgsl");
        match ty.1.inner {
            naga::TypeInner::Struct { span, .. } => assert_eq!(span as u64, UNIFORM_SIZE),
            ref other => panic!("Uniforms is not a struct: {other:?}"),
        }
    }

    /// GPU cost of each interpolation mode at 3840x2160 (roadmap 1.9). Needs a GPU, so it is
    /// ignored by `cargo test`. Run with
    /// `cargo test --release -p cv-render bench_modes_4k -- --ignored --nocapture`.
    ///
    /// Wall-clock time over many submitted frames, so it compares modes on one GPU rather than
    /// giving absolute GPU time. cleanEdge branches on content, so three frames are used:
    /// flat (no edges), stripes (hard diagonal edges) and noise (no two texels alike).
    #[test]
    #[ignore]
    fn bench_modes_4k() {
        const W: u32 = 3840;
        const H: u32 = 2160;
        const FRAMES: u32 = 300;

        let Some((device, queue, adapter_name)) = pollster::block_on(headless_device()) else {
            println!("no DX12 adapter; skipped");
            return;
        };
        println!("adapter: {adapter_name}");

        let format = wgpu::TextureFormat::Bgra8Unorm;
        let (pipeline, bgl) = WgpuState::make_pipeline(&device, format);
        let (frame_tex, frame_view) = WgpuState::make_frame_texture(&device, W, H);
        let sampler = WgpuState::make_sampler(&device);
        let uniform_buf = WgpuState::make_uniform_buf(&device);
        let bind_group = WgpuState::make_bind_group(&device, &bgl, &frame_view, &sampler, &uniform_buf);

        let target = device.create_texture(&wgpu::TextureDescriptor {
            label:           Some("bench-target"),
            size:            wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count:    1,
            dimension:       wgpu::TextureDimension::D2,
            format,
            usage:           wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats:    &[],
        });
        let target_view = target.create_view(&Default::default());

        let modes = [("Bilinear", 0u32), ("Bicubic", 1), ("Sharp", 2), ("CleanEdge", 3)];
        let edge_threshold = cv_core::AppState::default().edge_threshold;

        println!("{:<8} {:>5} {:>10} {:>10}", "frame", "zoom", "mode", "ms/frame");
        for (frame_name, pixels) in bench_frames(W, H) {
            queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture:   &frame_tex,
                    mip_level: 0,
                    origin:    wgpu::Origin3d::ZERO,
                    aspect:    wgpu::TextureAspect::All,
                },
                &pixels,
                wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(W * 4), rows_per_image: None },
                wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
            );
            for zoom in [10.0f32, 20.0] {
                let side = 1.0 / zoom;
                let crop = [0.5 - side / 2.0, 0.5 - side / 2.0, side, side];
                for (mode_name, mode) in modes {
                    let bytes = uniform_bytes(crop, 0, mode, 0, 0, edge_threshold);
                    queue.write_buffer(&uniform_buf, 0, &bytes);

                    // Warm up, then time.
                    for _ in 0..10 {
                        let mut enc = device.create_command_encoder(&Default::default());
                        encode_pass(&mut enc, &target_view, &pipeline, &bind_group);
                        queue.submit([enc.finish()]);
                    }
                    device.poll(wgpu::Maintain::Wait);

                    let start = Instant::now();
                    for _ in 0..FRAMES {
                        let mut enc = device.create_command_encoder(&Default::default());
                        encode_pass(&mut enc, &target_view, &pipeline, &bind_group);
                        queue.submit([enc.finish()]);
                    }
                    device.poll(wgpu::Maintain::Wait);
                    let ms = start.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;
                    println!("{frame_name:<8} {zoom:>5} {mode_name:>10} {ms:>10.3}");
                }
            }
        }
    }

    async fn headless_device() -> Option<(wgpu::Device, wgpu::Queue, String)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            ..Default::default()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference:       wgpu::PowerPreference::HighPerformance,
                compatible_surface:     None,
                force_fallback_adapter: false,
            })
            .await?;
        let name = adapter.get_info().name;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await
            .ok()?;
        Some((device, queue, name))
    }

    /// BGRA8 test frames: flat grey, hard diagonal black/white stripes, and noise.
    fn bench_frames(w: u32, h: u32) -> Vec<(&'static str, Vec<u8>)> {
        let n = (w * h) as usize;
        let flat = vec![0xEEu8; n * 4];

        let mut stripes = Vec::with_capacity(n * 4);
        for y in 0..h {
            for x in 0..w {
                let v = if ((x + y) / 3) % 2 == 0 { 0x00 } else { 0xFF };
                stripes.extend_from_slice(&[v, v, v, 0xFF]);
            }
        }

        let mut noise = Vec::with_capacity(n * 4);
        let mut s: u32 = 0x1234_5678;
        for _ in 0..n {
            // xorshift32
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            let [b, g, r, _] = s.to_le_bytes();
            noise.extend_from_slice(&[b, g, r, 0xFF]);
        }

        vec![("flat", flat), ("stripes", stripes), ("noise", noise)]
    }
}

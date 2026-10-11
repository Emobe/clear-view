use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::magnifier::{Renderer, Uniforms};

/// shader.wgsl with the cleanEdge port (MIT, see clean_edge.wgsl) appended. WGSL has no `#include`.
const SHADER: &str = concat!(include_str!("shader.wgsl"), "\n", include_str!("clean_edge.wgsl"));

/// Size of the `Uniforms` struct in shader.wgsl.
const UNIFORM_SIZE: u64 = 96;

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
    /// The pointer mask (ADR 0010), R8: 255 where the pointer is drawn. 1x1 and empty until the
    /// first shape arrives.
    mask_tex:       wgpu::Texture,
    mask_view:      wgpu::TextureView,
    pub tex_w:      u32,
    pub tex_h:      u32,
}

impl WgpuState {
    /// # Safety
    /// `display` and `window` must be valid, and must stay valid until this `WgpuState` is
    /// dropped (wgpu `SurfaceTargetUnsafe::RawHandle`).
    pub unsafe fn new(
        display: RawDisplayHandle,
        window: RawWindowHandle,
        win_w: u32,
        win_h: u32,
        tex_w: u32,
        tex_h: u32,
    ) -> Self {
        pollster::block_on(Self::init(display, window, win_w, win_h, tex_w, tex_h))
    }

    async fn init(
        display: RawDisplayHandle,
        window: RawWindowHandle,
        win_w: u32,
        win_h: u32,
        tex_w: u32,
        tex_h: u32,
    ) -> Self {
        let instance = dx12_instance();

        // SAFETY: the caller of `new` keeps both handles valid for the surface's lifetime.
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(display),
                raw_window_handle:  window,
            })
        }
        .expect("create_surface_unsafe failed");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference:       wgpu::PowerPreference::HighPerformance,
                compatible_surface:     Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets:    false,
            })
            .await
            .expect("No DX12 adapter found");

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label:              Some("cv-magnifier"),
                required_features:  wgpu::Features::empty(),
                required_limits:    wgpu::Limits::default(),
                memory_hints:       wgpu::MemoryHints::default(),
                ..Default::default()
            })
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
            // sRGB for Bgra8Unorm, as before wgpu 30 added the field.
            color_space:                    wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &surface_config);

        let (pipeline, bgl) = Self::make_pipeline(&device, surface_format);
        let (frame_tex, frame_view) = Self::make_frame_texture(&device, tex_w, tex_h);
        let sampler = Self::make_sampler(&device);
        let uniform_buf = Self::make_uniform_buf(&device);
        let (mask_tex, mask_view) = Self::make_mask_texture(&device, 1, 1);
        let bind_group =
            Self::make_bind_group(&device, &bgl, &frame_view, &sampler, &uniform_buf, &mask_view);

        Self {
            device, queue, surface, surface_config,
            pipeline, frame_tex, frame_view, sampler,
            uniform_buf, bgl, bind_group,
            mask_tex, mask_view,
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
                // binding 0 — Uniforms (96 bytes: crop + modes + cursor + edge threshold + pointer)
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
                // binding 3 — pointer mask texture (R8)
                wgpu::BindGroupLayoutEntry {
                    binding:    3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type:    wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled:   false,
                    },
                    count: None,
                },
            ],
        });

        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label:              Some("magnify-pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size:     0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:  Some("magnify"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module:              &shader,
                entry_point:         Some("vs"),
                buffers:             &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module:      &shader,
                entry_point: Some("fs"),
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
            multiview_mask: None,
            cache:          None,
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

    fn make_mask_texture(device: &wgpu::Device, w: u32, h: u32) -> (wgpu::Texture, wgpu::TextureView) {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label:           Some("pointer-mask"),
            size:            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count:    1,
            dimension:       wgpu::TextureDimension::D2,
            format:          wgpu::TextureFormat::R8Unorm,
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
        mask_view:   &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label:   Some("magnify-bg"),
            layout:  bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(frame_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(mask_view) },
            ],
        })
    }

    /// Replace the frame texture with one of a new size.
    /// Called when switching to a monitor with different resolution.
    pub fn recreate_frame_texture(&mut self, w: u32, h: u32) {
        let (frame_tex, frame_view) = Self::make_frame_texture(&self.device, w, h);
        self.bind_group = Self::make_bind_group(
            &self.device, &self.bgl, &frame_view, &self.sampler, &self.uniform_buf, &self.mask_view,
        );
        self.frame_tex  = frame_tex;
        self.frame_view = frame_view;
        self.tex_w = w;
        self.tex_h = h;
    }

    /// Put a pointer mask (`w * h` bytes, top-down) in the mask texture, replacing the texture
    /// first when the size differs. `write_texture` has no row alignment rule (wgpu-types
    /// `COPY_BYTES_PER_ROW_ALIGNMENT` is for buffer copies), so rows are `w` bytes.
    pub fn upload_pointer_mask(&mut self, data: &[u8], w: u32, h: u32) {
        if data.len() != w as usize * h as usize || w == 0 || h == 0 { return; }
        if (self.mask_tex.width(), self.mask_tex.height()) != (w, h) {
            let (mask_tex, mask_view) = Self::make_mask_texture(&self.device, w, h);
            self.bind_group = Self::make_bind_group(
                &self.device, &self.bgl, &self.frame_view, &self.sampler, &self.uniform_buf, &mask_view,
            );
            self.mask_tex  = mask_tex;
            self.mask_view = mask_view;
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture:   &self.mask_tex,
                mip_level: 0,
                origin:    wgpu::Origin3d::ZERO,
                aspect:    wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset:         0,
                bytes_per_row:  Some(w),
                rows_per_image: None,
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
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
            wgpu::TexelCopyTextureInfo {
                texture:   &self.frame_tex,
                mip_level: 0,
                origin:    wgpu::Origin3d::ZERO,
                aspect:    wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset:         0,
                bytes_per_row:  Some(w * 4),
                rows_per_image: None,
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    /// Write all uniforms to the 96-byte buffer; see `uniform_bytes` for the layout.
    pub fn write_uniforms(&self, u: &Uniforms) {
        self.queue.write_buffer(&self.uniform_buf, 0, &uniform_bytes(u));
    }

    /// Execute the render pass and present.
    /// Returns false on surface error (caller should call resize to recover).
    pub fn render(&self) -> bool {
        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(o) | wgpu::CurrentSurfaceTexture::Suboptimal(o) => o,
            other => {
                eprintln!("[render] surface error: {other:?}");
                return false;
            }
        };

        let view = output.texture.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        encode_pass(&mut enc, &view, &self.pipeline, &self.bind_group);
        self.queue.submit([enc.finish()]);
        self.queue.present(output);
        true
    }
}

impl Renderer for WgpuState {
    fn frame_size(&self) -> (u32, u32) {
        (self.tex_w, self.tex_h)
    }

    fn resize(&mut self, width: u32, height: u32) {
        WgpuState::resize(self, width, height);
    }

    fn recreate_frame_texture(&mut self, width: u32, height: u32) {
        WgpuState::recreate_frame_texture(self, width, height);
    }

    fn upload_frame(&mut self, data: &[u8], width: u32, height: u32) {
        WgpuState::upload_frame(self, data, width, height);
    }

    fn upload_pointer_mask(&mut self, data: &[u8], width: u32, height: u32) {
        WgpuState::upload_pointer_mask(self, data, width, height);
    }

    fn write_uniforms(&mut self, u: &Uniforms) {
        WgpuState::write_uniforms(self, u);
    }

    fn render(&mut self) -> bool {
        WgpuState::render(self)
    }
}

/// A DX12-only instance. FXC is set explicitly: wgpu 30's default picks DXC when
/// `dxcompiler.dll` is on the PATH, so the compiler would differ between machines.
fn dx12_instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        backend_options: wgpu::BackendOptions {
            dx12: wgpu::Dx12BackendOptions {
                shader_compiler: wgpu::Dx12Compiler::Fxc,
                ..Default::default()
            },
            ..Default::default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    })
}

/// The uniform buffer contents; layout matches `Uniforms` in shader.wgsl (the two `vec4`s sit
/// on 16-byte offsets, so there is no padding).
fn uniform_bytes(u: &Uniforms) -> [u8; UNIFORM_SIZE as usize] {
    let mut bytes = [0u8; UNIFORM_SIZE as usize];
    let mut put = |at: usize, v: [u8; 4]| bytes[at..at + 4].copy_from_slice(&v);
    for (i, f) in u.crop.iter().enumerate() {
        put(i * 4, f.to_ne_bytes());
    }
    put(16, u.color_mode.to_ne_bytes());
    put(20, u.interp_mode.to_ne_bytes());
    put(24, u.cursor_x.to_ne_bytes());
    put(28, u.cursor_y.to_ne_bytes());
    put(32, u.edge_threshold.to_ne_bytes());
    let p = &u.pointer;
    put(36, p.kind.to_ne_bytes());
    put(40, p.mask.0.to_ne_bytes());
    put(44, p.mask.1.to_ne_bytes());
    put(48, p.hotspot.0.to_ne_bytes());
    put(52, p.hotspot.1.to_ne_bytes());
    put(56, p.size.to_ne_bytes());
    put(60, p.outline.to_ne_bytes());
    for (i, f) in p.fill.iter().enumerate() {
        put(64 + i * 4, f.to_ne_bytes());
    }
    for (i, f) in p.outline_colour.iter().enumerate() {
        put(80 + i * 4, f.to_ne_bytes());
    }
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
            depth_slice:    None,
            resolve_target: None,
            ops: wgpu::Operations {
                load:  wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes:         None,
        occlusion_query_set:      None,
        multiview_mask:           None,
    });
    rpass.set_pipeline(pipeline);
    rpass.set_bind_group(0, bind_group, &[]);
    rpass.draw(0..6, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::magnifier::{CURSOR_HIDDEN, POINTER_NONE, POINTER_SHAPE, PointerDraw};
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
    /// `cargo test --release -p cv-magnifier bench_modes_4k -- --ignored --nocapture`.
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
        let (_mask_tex, mask_view) = WgpuState::make_mask_texture(&device, 1, 1);
        let bind_group =
            WgpuState::make_bind_group(&device, &bgl, &frame_view, &sampler, &uniform_buf, &mask_view);

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
                wgpu::TexelCopyTextureInfo {
                    texture:   &frame_tex,
                    mip_level: 0,
                    origin:    wgpu::Origin3d::ZERO,
                    aspect:    wgpu::TextureAspect::All,
                },
                &pixels,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(W * 4), rows_per_image: None },
                wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
            );
            for zoom in [10.0f32, 20.0] {
                let side = 1.0 / zoom;
                let crop = [0.5 - side / 2.0, 0.5 - side / 2.0, side, side];
                for (mode_name, mode) in modes {
                    let uniforms = Uniforms {
                        crop,
                        color_mode: 0,
                        interp_mode: mode,
                        cursor_x: CURSOR_HIDDEN,
                        cursor_y: CURSOR_HIDDEN,
                        edge_threshold,
                        pointer: PointerDraw::marker(POINTER_NONE),
                    };
                    queue.write_buffer(&uniform_buf, 0, &uniform_bytes(&uniforms));

                    // Warm up, then time.
                    for _ in 0..10 {
                        let mut enc = device.create_command_encoder(&Default::default());
                        encode_pass(&mut enc, &target_view, &pipeline, &bind_group);
                        queue.submit([enc.finish()]);
                    }
                    device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");

                    let start = Instant::now();
                    for _ in 0..FRAMES {
                        let mut enc = device.create_command_encoder(&Default::default());
                        encode_pass(&mut enc, &target_view, &pipeline, &bind_group);
                        queue.submit([enc.finish()]);
                    }
                    device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
                    let ms = start.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;
                    println!("{frame_name:<8} {zoom:>5} {mode_name:>10} {ms:>10.3}");
                }
            }
        }
    }

    /// Cost of uploading one 3840x2160 BGRA8 frame (roadmap 5.5a), the work `upload_frame` does
    /// for every new capture frame. Needs a GPU, so it is ignored by `cargo test`. Run with
    /// `cargo test --release -p cv-magnifier bench_upload_4k -- --ignored --nocapture`, with the
    /// GPU idle.
    ///
    /// Two numbers: the `write_texture` call alone (what the render thread waits for in a tick,
    /// the copy into wgpu's staging memory) and the whole upload to the point the GPU copy has
    /// finished (`submit` and wait).
    #[test]
    #[ignore]
    fn bench_upload_4k() {
        const W: u32 = 3840;
        const H: u32 = 2160;
        const FRAMES: u32 = 100;

        let Some((device, queue, adapter_name)) = pollster::block_on(headless_device()) else {
            println!("no DX12 adapter; skipped");
            return;
        };
        println!("adapter: {adapter_name}");

        let (frame_tex, _view) = WgpuState::make_frame_texture(&device, W, H);
        let pixels = vec![0xEEu8; (W * H * 4) as usize];
        let upload = || {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture:   &frame_tex,
                    mip_level: 0,
                    origin:    wgpu::Origin3d::ZERO,
                    aspect:    wgpu::TextureAspect::All,
                },
                &pixels,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(W * 4), rows_per_image: None },
                wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
            );
        };

        // Warm up.
        for _ in 0..5 {
            upload();
            queue.submit([]);
        }
        device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");

        let mut call = std::time::Duration::ZERO;
        let mut call_max = std::time::Duration::ZERO;
        let start = Instant::now();
        for _ in 0..FRAMES {
            let t = Instant::now();
            upload();
            let d = t.elapsed();
            call += d;
            call_max = call_max.max(d);
            queue.submit([]);
            device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        }
        let total = start.elapsed();
        println!("{W}x{H} frame, {} MB", W * H * 4 / 1_000_000);
        println!(
            "write_texture call: avg {:.2} ms, max {:.2} ms",
            call.as_secs_f64() * 1000.0 / FRAMES as f64,
            call_max.as_secs_f64() * 1000.0,
        );
        println!(
            "upload to GPU copy finished: avg {:.2} ms",
            total.as_secs_f64() * 1000.0 / FRAMES as f64,
        );
    }

    /// The pointer shader (roadmap 6.2c2) on a tiny offscreen target: a 4x4 all-drawn mask with
    /// its hotspot at (1, 1), drawn at 2 output pixels per mask pixel with a 2 px outline, at
    /// output pixel (16, 16). Needs a GPU, so it is ignored by `cargo test`. Run with
    /// `cargo test -p cv-magnifier pointer_shader_draws_the_mask -- --ignored --nocapture`.
    ///
    /// The mask covers output pixels 14..22 on both axes; the outline adds two more on each side
    /// (12, 13 and 22, 23), and the disc leaves the corner (12, 12) out.
    #[test]
    #[ignore]
    fn pointer_shader_draws_the_mask() {
        const SIDE: u32 = 32;
        const ROW: u32 = 256; // SIDE * 4 bytes, padded to COPY_BYTES_PER_ROW_ALIGNMENT

        let Some((device, queue, adapter_name)) = pollster::block_on(headless_device()) else {
            println!("no DX12 adapter; skipped");
            return;
        };
        println!("adapter: {adapter_name}");

        let format = wgpu::TextureFormat::Bgra8Unorm;
        let (pipeline, bgl) = WgpuState::make_pipeline(&device, format);
        let (_frame_tex, frame_view) = WgpuState::make_frame_texture(&device, 4, 4);
        let sampler = WgpuState::make_sampler(&device);
        let uniform_buf = WgpuState::make_uniform_buf(&device);
        let (mask_tex, mask_view) = WgpuState::make_mask_texture(&device, 4, 4);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture:   &mask_tex,
                mip_level: 0,
                origin:    wgpu::Origin3d::ZERO,
                aspect:    wgpu::TextureAspect::All,
            },
            &[255u8; 16],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: None },
            wgpu::Extent3d { width: 4, height: 4, depth_or_array_layers: 1 },
        );
        let bind_group =
            WgpuState::make_bind_group(&device, &bgl, &frame_view, &sampler, &uniform_buf, &mask_view);

        let target = device.create_texture(&wgpu::TextureDescriptor {
            label:           Some("pointer-target"),
            size:            wgpu::Extent3d { width: SIDE, height: SIDE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count:    1,
            dimension:       wgpu::TextureDimension::D2,
            format,
            usage:           wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats:    &[],
        });
        let target_view = target.create_view(&Default::default());
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label:              Some("pointer-readback"),
            size:               (ROW * SIDE) as u64,
            usage:              wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        // Renders once and returns the target as BGRA rows of `ROW` bytes.
        let render = |uniforms: &Uniforms| -> Vec<u8> {
            queue.write_buffer(&uniform_buf, 0, &uniform_bytes(uniforms));
            let mut enc = device.create_command_encoder(&Default::default());
            encode_pass(&mut enc, &target_view, &pipeline, &bind_group);
            enc.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture:   &target,
                    mip_level: 0,
                    origin:    wgpu::Origin3d::ZERO,
                    aspect:    wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset:         0,
                        bytes_per_row:  Some(ROW),
                        rows_per_image: None,
                    },
                },
                wgpu::Extent3d { width: SIDE, height: SIDE, depth_or_array_layers: 1 },
            );
            queue.submit([enc.finish()]);
            readback.slice(..).map_async(wgpu::MapMode::Read, |r| r.expect("map"));
            device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
            let bytes = readback.slice(..).get_mapped_range().expect("mapped range").to_vec();
            readback.unmap();
            bytes
        };
        let rgb = |bytes: &[u8], x: u32, y: u32| {
            let at = (y * ROW + x * 4) as usize;
            [bytes[at + 2], bytes[at + 1], bytes[at]]
        };

        let pointer = PointerDraw {
            kind: POINTER_SHAPE,
            mask: (4, 4),
            hotspot: (1, 1),
            size: 2.0,
            outline: 2.0,
            fill: [1.0, 0.0, 0.0, 1.0],
            outline_colour: [1.0, 1.0, 1.0, 1.0],
        };
        let uniforms = Uniforms {
            crop: [0.0, 0.0, 1.0, 1.0],
            color_mode: 0,
            interp_mode: 0,
            cursor_x: 16,
            cursor_y: 16,
            edge_threshold: 0.1,
            pointer,
        };

        let out = render(&uniforms);
        let (red, white, black) = ([255, 0, 0], [255, 255, 255], [0, 0, 0]);
        assert_eq!(rgb(&out, 16, 16), red, "inside the mask");
        assert_eq!(rgb(&out, 14, 14), red, "mask corner, the first pixel left and above the hotspot");
        assert_eq!(rgb(&out, 21, 21), red, "mask corner, last pixel");
        assert_eq!(rgb(&out, 13, 16), white, "outline, left");
        assert_eq!(rgb(&out, 12, 16), white, "outline, two pixels out");
        assert_eq!(rgb(&out, 11, 16), black, "past the outline");
        assert_eq!(rgb(&out, 22, 16), white, "outline, right");
        assert_eq!(rgb(&out, 23, 16), white, "outline, two pixels out on the right");
        assert_eq!(rgb(&out, 24, 16), black, "past the outline, right");
        assert_eq!(rgb(&out, 13, 13), white, "outline, diagonal");
        assert_eq!(rgb(&out, 12, 12), black, "the disc leaves this corner out");
        assert_eq!(rgb(&out, 2, 2), black, "far from the pointer");

        let out = render(&Uniforms { cursor_x: CURSOR_HIDDEN, cursor_y: CURSOR_HIDDEN, ..uniforms });
        assert_eq!(rgb(&out, 16, 16), black, "out of view draws nothing");

        let out = render(&Uniforms { pointer: PointerDraw::marker(POINTER_NONE), ..uniforms });
        assert_eq!(rgb(&out, 16, 16), black, "kind none draws nothing");

        let circle = PointerDraw::marker(crate::magnifier::POINTER_CIRCLE);
        let out = render(&Uniforms { pointer: circle, ..uniforms });
        assert_eq!(rgb(&out, 16, 16), white, "the circle fallback");
        assert_eq!(rgb(&out, 16, 27), black, "the circle's ring");
    }

    async fn headless_device() -> Option<(wgpu::Device, wgpu::Queue, String)> {
        let instance = dx12_instance();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference:       wgpu::PowerPreference::HighPerformance,
                compatible_surface:     None,
                force_fallback_adapter: false,
                apply_limit_buckets:    false,
            })
            .await
            .ok()?;
        let name = adapter.get_info().name;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
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

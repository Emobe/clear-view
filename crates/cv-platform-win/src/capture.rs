use std::{
    mem::size_of,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use cv_core::{Frame, OutputInfo};
use cv_magnifier::{CaptureError, CaptureSource};
use windows::{
    core::Interface,
    Win32::Foundation::RECT,
    Win32::Graphics::{
        Direct3D::D3D_DRIVER_TYPE_HARDWARE,
        Direct3D11::{
            D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
            D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
            D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
        },
        Dxgi::{
            Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC},
            IDXGIAdapter, IDXGIDevice, IDXGIOutput1, IDXGIOutput2, IDXGIOutputDuplication,
            DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET,
            DXGI_ERROR_MORE_DATA, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
        },
    },
};

use crate::proto;

struct D3dCtx {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
}

pub struct Capturer {
    ctx: D3dCtx,
    /// `None` after `recreate` released a lost duplication and could not make a new one.
    duplication: Option<IDXGIOutputDuplication>,
    staging: ID3D11Texture2D,
    width: u32,
    height: u32,
    output_idx: u32,
    /// 3.1 prototype: the output's top-left in virtual-screen pixels, to map the overlay rect
    /// into frame space.
    origin: (i32, i32),
    stats: CaptureStats,
    dirty: Vec<RECT>,
}

impl Capturer {
    /// Creates the D3D11 device and duplicates output `output_idx`. Call it on the thread that
    /// will use the capturer (the factory passed to `cv_magnifier::spawn_capture`).
    pub fn new_for_output(output_idx: u32) -> Result<Self, CaptureError> {
        Self::create(output_idx).map_err(capture_error)
    }

    fn create(output_idx: u32) -> windows::core::Result<Self> {
        let ctx = create_device()?;
        let dup = create_duplication(&ctx.device, output_idx)?;
        let staging = create_staging(&ctx.device, dup.width, dup.height)?;
        Ok(Self {
            ctx,
            duplication: Some(dup.duplication),
            staging,
            width: dup.width,
            height: dup.height,
            output_idx,
            origin: dup.origin,
            stats: CaptureStats::new(),
            dirty: Vec::new(),
        })
    }

    /// Returns `None` on timeout (no new frame yet), `Err` on device loss.
    fn acquire(&mut self, timeout_ms: u32) -> windows::core::Result<Option<Frame>> {
        self.stats.report_if_due();
        // No duplication: the last rebuild failed, so report it lost and let the loop retry.
        let Some(duplication) = &self.duplication else {
            return Err(DXGI_ERROR_ACCESS_LOST.into());
        };
        unsafe {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource = None;

            match duplication.AcquireNextFrame(timeout_ms, &mut info, &mut resource) {
                Ok(_) => {}
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(None),
                Err(e) => return Err(e),
            }

            // 3.1 prototype: classify the frame before copying it.
            let image = info.LastPresentTime != 0;
            let dirty = if image { dirty_rects(duplication, &mut self.dirty)? } else { &[][..] };
            let overlay = proto::overlay().map(|r| RECT {
                left: r.left - self.origin.0,
                top: r.top - self.origin.1,
                right: r.right - self.origin.0,
                bottom: r.bottom - self.origin.1,
            });
            self.stats.record_frame(image, info.AccumulatedFrames, dirty, overlay.as_ref());

            let copy_started = Instant::now();
            let texture: ID3D11Texture2D = resource.unwrap().cast()?;
            self.ctx.context.CopyResource(&self.staging, &texture);
            let data = read_staging(&self.ctx.context, &self.staging, self.width, self.height)?;
            duplication.ReleaseFrame()?;
            self.stats.copy += copy_started.elapsed();

            Ok(Some(Frame { width: self.width, height: self.height, data }))
        }
    }

    /// Switch to capturing a different output (monitor). Recreates duplication + staging.
    fn duplicate(&mut self, idx: u32) -> windows::core::Result<()> {
        let dup = create_duplication(&self.ctx.device, idx)?;
        let staging = create_staging(&self.ctx.device, dup.width, dup.height)?;
        self.duplication = Some(dup.duplication);
        self.staging = staging;
        self.width = dup.width;
        self.height = dup.height;
        self.output_idx = idx;
        self.origin = dup.origin;
        Ok(())
    }

    /// Rebuilds device, duplication and staging for the current output. The old duplication is
    /// released first: after `DXGI_ERROR_ACCESS_LOST` the docs say to release it before
    /// creating a new one, and `DuplicateOutput` fails with `E_INVALIDARG` while this process
    /// still duplicates the output.
    fn recreate(&mut self) -> windows::core::Result<()> {
        self.duplication = None;
        *self = Self::create(self.output_idx)?;
        Ok(())
    }
}

/// The dirty rects of the frame currently held, in frame pixels. `buf` is reused between frames.
unsafe fn dirty_rects<'a>(
    duplication: &IDXGIOutputDuplication,
    buf: &'a mut Vec<RECT>,
) -> windows::core::Result<&'a [RECT]> {
    let rect_size = size_of::<RECT>() as u32;
    loop {
        let mut required = 0u32;
        let result = unsafe {
            duplication.GetFrameDirtyRects(
                buf.len() as u32 * rect_size,
                buf.as_mut_ptr(),
                &mut required,
            )
        };
        match result {
            Ok(()) => return Ok(&buf[..(required / rect_size) as usize]),
            Err(e) if e.code() == DXGI_ERROR_MORE_DATA => {
                buf.resize((required / rect_size) as usize, RECT::default());
            }
            Err(e) => return Err(e),
        }
    }
}

/// Once-a-second capture report for the 3.1 prototype.
struct CaptureStats {
    since: Instant,
    /// Image updates (`LastPresentTime` non-zero).
    image: u32,
    /// Pointer-only updates: no new desktop image, the frame is copied anyway.
    pointer_only: u32,
    /// Image updates whose dirty rects all lie inside the overlay: suspected extra frames.
    overlay_only: u32,
    /// Image updates with at least one dirty rect touching the overlay.
    touching_overlay: u32,
    /// Image updates that reported no dirty rects at all.
    no_rects: u32,
    accumulated: u32,
    dirty_px: u64,
    copy: Duration,
}

impl CaptureStats {
    fn new() -> Self {
        Self {
            since: Instant::now(),
            image: 0,
            pointer_only: 0,
            overlay_only: 0,
            touching_overlay: 0,
            no_rects: 0,
            accumulated: 0,
            dirty_px: 0,
            copy: Duration::ZERO,
        }
    }

    fn record_frame(&mut self, image: bool, accumulated: u32, dirty: &[RECT], overlay: Option<&RECT>) {
        if !image {
            self.pointer_only += 1;
            return;
        }
        self.image += 1;
        self.accumulated += accumulated;
        if dirty.is_empty() {
            self.no_rects += 1;
        }
        self.dirty_px += dirty
            .iter()
            .map(|r| ((r.right - r.left).max(0) as u64) * ((r.bottom - r.top).max(0) as u64))
            .sum::<u64>();
        if let Some(o) = overlay {
            if !dirty.is_empty() && dirty.iter().all(|r| proto::contains(o, r)) {
                self.overlay_only += 1;
            }
            if dirty.iter().any(|r| proto::intersects(o, r)) {
                self.touching_overlay += 1;
            }
        }
    }

    fn report_if_due(&mut self) {
        let wall = self.since.elapsed();
        if wall < Duration::from_secs(1) {
            return;
        }
        let frames = self.image + self.pointer_only;
        let copy_ms = self.copy.as_secs_f64() * 1000.0;
        eprintln!(
            "[proto] capture: {:.1} s, frames {frames} (image {}, pointer-only {}), image frames \
             with dirty rects only inside overlay {}, touching overlay {}, with no rects {}, \
             accumulated {}, dirty {:.2} Mpx, copy {copy_ms:.1} ms total ({:.2} ms per frame)",
            wall.as_secs_f64(),
            self.image,
            self.pointer_only,
            self.overlay_only,
            self.touching_overlay,
            self.no_rects,
            self.accumulated,
            self.dirty_px as f64 / 1e6,
            copy_ms / f64::from(frames.max(1)),
        );
        *self = Self::new();
    }
}

impl CaptureSource for Capturer {
    fn next_frame(&mut self, timeout_ms: u32) -> Result<Option<Frame>, CaptureError> {
        self.acquire(timeout_ms).map_err(capture_error)
    }

    fn switch_output(&mut self, idx: u32) -> Result<(), CaptureError> {
        self.duplicate(idx).map_err(capture_error)
    }

    /// Rebuilds the device and the duplication whatever the error was.
    fn reconnect(&mut self) -> Result<(), CaptureError> {
        self.recreate().map_err(capture_error)
    }
}

fn capture_error(e: windows::core::Error) -> CaptureError {
    let message = e.to_string();
    match e.code() {
        DXGI_ERROR_ACCESS_LOST => CaptureError::AccessLost(message),
        DXGI_ERROR_DEVICE_REMOVED | DXGI_ERROR_DEVICE_RESET => CaptureError::DeviceLost(message),
        _ => CaptureError::Other(message),
    }
}

/// Enumerate all monitors attached to the primary adapter.
/// Returns one `OutputInfo` per active output, in DXGI output order.
pub fn enumerate_outputs() -> Vec<OutputInfo> {
    unsafe {
        let ctx = match create_device() {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let dxgi: IDXGIDevice = match ctx.device.cast() {
            Ok(d) => d,
            Err(_) => return vec![],
        };
        let adapter = match dxgi.GetAdapter() {
            Ok(a) => a,
            Err(_) => return vec![],
        };

        let mut result = Vec::new();
        let mut idx = 0u32;
        loop {
            let output = match adapter.EnumOutputs(idx) {
                Ok(o)  => o,
                Err(_) => break,
            };
            if let Ok(desc) = output.GetDesc()
                && desc.AttachedToDesktop.as_bool()
            {
                let r = desc.DesktopCoordinates;
                result.push(OutputInfo {
                    idx,
                    left:   r.left,
                    top:    r.top,
                    width:  (r.right  - r.left) as u32,
                    height: (r.bottom - r.top)  as u32,
                });
            }
            idx += 1;
        }

        // Always have at least one entry so callers never see an empty list.
        if result.is_empty() {
            result.push(OutputInfo { idx: 0, left: 0, top: 0, width: 1920, height: 1080 });
        }
        result
    }
}

fn create_device() -> windows::core::Result<D3dCtx> {
    unsafe {
        let mut device = None;
        let mut context = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            windows::Win32::Foundation::HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
        Ok(D3dCtx { device: device.unwrap(), context: context.unwrap() })
    }
}

struct Duplication {
    duplication: IDXGIOutputDuplication,
    width: u32,
    height: u32,
    /// The output's top-left in virtual-screen pixels.
    origin: (i32, i32),
}

/// 3.1 prototype: the multiplane overlay support of each output is logged once per process.
static MPO_LOGGED: AtomicBool = AtomicBool::new(false);

fn create_duplication(
    device: &ID3D11Device,
    output_idx: u32,
) -> windows::core::Result<Duplication> {
    unsafe {
        let dxgi: IDXGIDevice = device.cast()?;
        let adapter = dxgi.GetAdapter()?;
        let output = adapter.EnumOutputs(output_idx)?;
        let coords = output.GetDesc()?.DesktopCoordinates;
        if !MPO_LOGGED.swap(true, Ordering::Relaxed) {
            log_mpo_support(&adapter);
        }
        let output1: IDXGIOutput1 = output.cast()?;
        let dup = output1.DuplicateOutput(device)?;
        let desc = dup.GetDesc();
        Ok(Duplication {
            duplication: dup,
            width: desc.ModeDesc.Width,
            height: desc.ModeDesc.Height,
            origin: (coords.left, coords.top),
        })
    }
}

/// Logs `IDXGIOutput2::SupportsOverlays` for every output: the 24H2 extra-frame issue was seen
/// only on outputs with multiplane overlay support.
fn log_mpo_support(adapter: &IDXGIAdapter) {
    let mut idx = 0;
    while let Ok(output) = unsafe { adapter.EnumOutputs(idx) } {
        let mpo = output
            .cast::<IDXGIOutput2>()
            .map(|o| unsafe { o.SupportsOverlays() }.as_bool());
        let name = unsafe { output.GetDesc() }
            .map(|d| String::from_utf16_lossy(&d.DeviceName).trim_end_matches('\0').to_string())
            .unwrap_or_default();
        match mpo {
            Ok(m) => eprintln!("[proto] output {idx} {name}: multiplane overlay support {m}"),
            Err(e) => eprintln!("[proto] output {idx} {name}: IDXGIOutput2 unavailable: {e}"),
        }
        idx += 1;
    }
}

fn create_staging(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> windows::core::Result<ID3D11Texture2D> {
    unsafe {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            ..Default::default()
        };
        let mut tex = None;
        device.CreateTexture2D(&desc, None, Some(&mut tex))?;
        Ok(tex.unwrap())
    }
}

fn read_staging(
    ctx: &ID3D11DeviceContext,
    staging: &ID3D11Texture2D,
    width: u32,
    height: u32,
) -> windows::core::Result<Vec<u8>> {
    unsafe {
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        ctx.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;

        let row_pitch = mapped.RowPitch as usize;
        let w = width as usize;
        let h = height as usize;
        let mut data = vec![0u8; w * h * 4];
        let src = std::slice::from_raw_parts(mapped.pData as *const u8, row_pitch * h);

        for row in 0..h {
            let src_row = &src[row * row_pitch..row * row_pitch + w * 4];
            let dst_row = &mut data[row * w * 4..(row + 1) * w * 4];
            dst_row.copy_from_slice(src_row);
        }

        ctx.Unmap(staging, 0);
        Ok(data)
    }
}

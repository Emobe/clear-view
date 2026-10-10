//! Startup log (ADR 0007): the Windows build and each output's multiplane overlay (MPO) support.
//! The 24H2 issue where a capture-excluded overlay still causes new frames was seen only on
//! outputs with MPO and reported fixed in build 26100.2314, so these lines are read on the
//! tester's machine before tester build 2 (roadmap 3.6). Each output line also gives the
//! display scale and desktop rect (roadmap 5.3).

use windows::{
    Win32::{
        Graphics::Dxgi::{DXGI_OUTPUT_DESC, IDXGIOutput2},
        System::Registry::{
            HKEY_LOCAL_MACHINE, REG_ROUTINE_FLAGS, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
        },
        UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
    },
    core::{Interface, PCWSTR, w},
};

use crate::capture::primary_adapter;

/// `GetVersionExW` reports 6.2 to a process without a compatibility manifest, and
/// `RtlGetVersion` has no update revision (the 2314 in 26100.2314), so the build is read here.
const CURRENT_VERSION: PCWSTR = w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");

/// Prints the Windows build, then one line per output on the primary adapter, to stderr.
pub fn log_system_info() {
    eprintln!("[system] {}", windows_version());
    log_outputs();
}

/// For example "Windows 25H2 build 26200.9457".
fn windows_version() -> String {
    let build = reg_string(w!("CurrentBuildNumber"))
        .and_then(|b| reg_dword(w!("UBR")).map(|ubr| format!("{b}.{ubr}")));
    // DisplayVersion is missing before 20H2; the build is what matters.
    let release = reg_string(w!("DisplayVersion")).map(|v| format!(" {v}")).unwrap_or_default();
    match build {
        Ok(build) => format!("Windows{release} build {build}"),
        Err(e) => format!("Windows{release} build unknown: {e}"),
    }
}

fn reg_string(name: PCWSTR) -> windows::core::Result<String> {
    let mut bytes = 0u32;
    unsafe { get_value(name, RRF_RT_REG_SZ, None, &mut bytes) }?;
    let mut buf = vec![0u16; (bytes as usize).div_ceil(2)];
    unsafe { get_value(name, RRF_RT_REG_SZ, Some(buf.as_mut_ptr().cast()), &mut bytes) }?;
    // `bytes` now counts the terminating null.
    buf.truncate((bytes as usize / 2).saturating_sub(1));
    Ok(String::from_utf16_lossy(&buf))
}

fn reg_dword(name: PCWSTR) -> windows::core::Result<u32> {
    let mut value = 0u32;
    let mut bytes = size_of::<u32>() as u32;
    unsafe {
        get_value(name, RRF_RT_REG_DWORD, Some((&raw mut value).cast()), &mut bytes)?;
    }
    Ok(value)
}

/// # Safety
/// `data`, when given, must point to at least `*bytes` writable bytes.
unsafe fn get_value(
    name: PCWSTR,
    flags: REG_ROUTINE_FLAGS,
    data: Option<*mut core::ffi::c_void>,
    bytes: &mut u32,
) -> windows::core::Result<()> {
    unsafe {
        RegGetValueW(HKEY_LOCAL_MACHINE, CURRENT_VERSION, name, flags, None, data, Some(bytes))
    }
    .ok()
}

/// `IDXGIOutput2::SupportsOverlays`, display scale and desktop rect for every output of the
/// adapter capture uses. The scale and rect show whether a test ran with mixed scaling across
/// monitors (roadmap 5.3); the rect is in physical pixels because the process is Per-Monitor v2.
fn log_outputs() {
    let adapter = match primary_adapter() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[system] multiplane overlay support unknown: no adapter: {e}");
            return;
        }
    };
    let mut idx = 0;
    while let Ok(output) = unsafe { adapter.EnumOutputs(idx) } {
        let desc = unsafe { output.GetDesc() }.ok();
        let name = desc
            .as_ref()
            .map(|d| String::from_utf16_lossy(&d.DeviceName).trim_end_matches('\0').to_string())
            .unwrap_or_default();
        let mpo = match output.cast::<IDXGIOutput2>() {
            Ok(o) => unsafe { o.SupportsOverlays() }.as_bool().to_string(),
            Err(e) => format!("unknown: {e}"),
        };
        let place = desc.as_ref().map(placement).unwrap_or_default();
        eprintln!("[system] output {idx} {name}: multiplane overlay support {mpo}{place}");
        idx += 1;
    }
}

/// ", scale 150%, at (2560, 0) 1920x1080"; the scale is left out if the monitor has none.
fn placement(desc: &DXGI_OUTPUT_DESC) -> String {
    let r = desc.DesktopCoordinates;
    let (mut dpi, mut dpi_y) = (0, 0);
    // A Per-Monitor aware caller gets the scale the user set for that display (Microsoft docs).
    let scale = unsafe { GetDpiForMonitor(desc.Monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y) }
        .map(|()| format!(", scale {}%", scale_percent(dpi)))
        .unwrap_or_default();
    format!(
        "{scale}, at ({}, {}) {}x{}",
        r.left,
        r.top,
        r.right - r.left,
        r.bottom - r.top
    )
}

/// The display scale as Windows Settings shows it: 96 DPI is 100%.
fn scale_percent(dpi: u32) -> u32 {
    (dpi * 100 + 48) / 96
}

#[cfg(test)]
mod tests {
    use super::scale_percent;

    #[test]
    fn scale_percent_matches_the_settings_values() {
        for (dpi, percent) in [(96, 100), (120, 125), (144, 150), (168, 175), (192, 200), (240, 250)] {
            assert_eq!(scale_percent(dpi), percent, "{dpi} DPI");
        }
    }
}

//! Startup log (ADR 0007): the Windows build and each output's multiplane overlay (MPO) support.
//! The 24H2 issue where a capture-excluded overlay still causes new frames was seen only on
//! outputs with MPO and reported fixed in build 26100.2314, so these lines are read on the
//! tester's machine before tester build 2 (roadmap 3.6).

use windows::{
    Win32::{
        Graphics::Dxgi::IDXGIOutput2,
        System::Registry::{
            HKEY_LOCAL_MACHINE, REG_ROUTINE_FLAGS, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
        },
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
    log_overlay_support();
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

/// `IDXGIOutput2::SupportsOverlays` for every output of the adapter capture uses.
fn log_overlay_support() {
    let adapter = match primary_adapter() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[system] multiplane overlay support unknown: no adapter: {e}");
            return;
        }
    };
    let mut idx = 0;
    while let Ok(output) = unsafe { adapter.EnumOutputs(idx) } {
        let name = unsafe { output.GetDesc() }
            .map(|d| String::from_utf16_lossy(&d.DeviceName).trim_end_matches('\0').to_string())
            .unwrap_or_default();
        match output.cast::<IDXGIOutput2>() {
            Ok(o) => {
                let mpo = unsafe { o.SupportsOverlays() }.as_bool();
                eprintln!("[system] output {idx} {name}: multiplane overlay support {mpo}");
            }
            Err(e) => {
                eprintln!("[system] output {idx} {name}: multiplane overlay support unknown: {e}")
            }
        }
        idx += 1;
    }
}

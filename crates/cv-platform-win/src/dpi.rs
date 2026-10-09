use windows::Win32::UI::HiDpi::{
    AreDpiAwarenessContextsEqual, GetThreadDpiAwarenessContext, SetProcessDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

/// Call first thing in `main`, before any window or thread exists.
///
/// Per-Monitor v2 makes the cursor, monitor rects, captured frames and window rects all
/// physical pixels. The call fails if awareness is already set, so check what took effect:
/// anything else means Windows scales some of those values and the view and circle drift.
pub fn set_dpi_awareness() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        if !AreDpiAwarenessContextsEqual(
            GetThreadDpiAwarenessContext(),
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )
        .as_bool()
        {
            eprintln!(
                "[dpi] process is not Per-Monitor v2 DPI aware; the view and cursor circle \
                 may not line up at display scaling above 100%"
            );
        }
    }
}

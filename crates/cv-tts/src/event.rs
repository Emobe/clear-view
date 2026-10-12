//! A Win32 auto-reset event. The speech thread (8.8b, ADR 0013) waits on the events of its
//! backends; a WinRT completion callback running on another thread signals one.

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Threading::{CreateEventW, SetEvent};

pub struct Event(HANDLE);

// A Win32 event handle may be signalled and waited on from any thread.
unsafe impl Send for Event {}
unsafe impl Sync for Event {}

impl Event {
    pub fn new() -> Result<Self, String> {
        unsafe { CreateEventW(None, false, false, None) }
            .map(Self)
            .map_err(|e| format!("CreateEventW failed: {e}"))
    }

    pub fn set(&self) {
        // Failing to signal a live event handle cannot be handled here; the waiter has a timeout.
        let _ = unsafe { SetEvent(self.0) };
    }

    pub fn handle(&self) -> HANDLE {
        self.0
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

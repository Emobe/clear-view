//! The magnifier (ADR 0004): the per-tick logic, the wgpu pipeline and its shaders, the capture
//! loop, and the traits the platform backend implements. Platform-neutral: the backend creates
//! the window, hands over raw handles and calls `Magnifier::tick`.

mod capture;
mod gfx;
mod host;
mod magnifier;

pub use capture::{CaptureError, CaptureSource, spawn_capture};
pub use host::{Layout, OverlayHost};
pub use magnifier::Magnifier;

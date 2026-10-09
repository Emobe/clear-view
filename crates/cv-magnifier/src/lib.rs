//! The magnifier (ADR 0004): the wgpu pipeline and its shaders, the capture loop, and the
//! traits the platform backend implements. Platform-neutral: the backend creates the window
//! and hands over raw handles.

mod capture;
mod gfx;
mod host;

pub use capture::{CaptureError, CaptureSource, spawn_capture};
pub use gfx::WgpuState;
pub use host::{Layout, OverlayHost};

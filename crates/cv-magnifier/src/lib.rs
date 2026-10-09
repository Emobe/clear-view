//! The magnifier (ADR 0004): the wgpu pipeline and its shaders. Platform-neutral: the backend
//! creates the window and hands over raw handles.

mod gfx;

pub use gfx::WgpuState;

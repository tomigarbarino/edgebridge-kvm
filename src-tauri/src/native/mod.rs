mod capture;
mod injection;

pub use capture::Capture;
pub use injection::WindowsDesktop;

use crate::state::Screen;
use windows_sys::Win32::UI::{
    HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
        SetThreadDpiAwarenessContext,
    },
    WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN},
};

pub fn configure_dpi() {
    // The application manifest or WebView may already have set process awareness.
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

fn screen() -> Screen {
    unsafe {
        let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let screen = Screen {
            width: GetSystemMetrics(SM_CXSCREEN).max(1),
            height: GetSystemMetrics(SM_CYSCREEN).max(1),
        };
        if !previous.is_null() {
            SetThreadDpiAwarenessContext(previous);
        }
        screen
    }
}

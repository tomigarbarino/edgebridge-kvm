mod capture;
mod injection;
mod interfaces;

pub use capture::Capture;
pub use injection::WindowsDesktop;
pub use interfaces::lan_addresses;

use std::{
    cell::RefCell,
    ptr::{null, null_mut},
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    layout::{Layout, Monitor},
    state::Screen,
};
use windows_sys::Win32::{
    Foundation::{LPARAM, RECT},
    Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR},
    UI::{
        HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
            SetThreadDpiAwarenessContext,
        },
        WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN},
    },
};

thread_local! {
    static LAYOUT: RefCell<Option<(Instant, Arc<Layout>)>> = const { RefCell::new(None) };
}

pub fn configure_dpi() {
    // The application manifest or WebView may already have set process awareness.
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

#[tauri::command]
pub fn restore_cursor() -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_SETCURSORS, SystemParametersInfoW};

    if unsafe { SystemParametersInfoW(SPI_SETCURSORS, 0, null_mut(), 0) } == 0 {
        return Err(format!(
            "Windows could not reload its cursors. {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn layout() -> Arc<Layout> {
    LAYOUT.with_borrow_mut(|cache| {
        let now = Instant::now();
        if let Some((updated, layout)) = cache
            && now.duration_since(*updated) < Duration::from_millis(250)
        {
            return Arc::clone(layout);
        }
        let mut monitors = Vec::new();
        let detected = enumerate(&mut monitors);
        let next = Layout::new(monitors);
        if let Some((updated, existing)) = cache
            && (!detected || next.as_ref().is_none_or(|next| next == existing.as_ref()))
        {
            *updated = now;
            return Arc::clone(existing);
        }
        let layout = Arc::new(next.filter(|_| detected).unwrap_or_else(|| {
            Layout::single(Screen {
                width: unsafe { GetSystemMetrics(SM_CXSCREEN) },
                height: unsafe { GetSystemMetrics(SM_CYSCREEN) },
            })
        }));
        *cache = Some((now, Arc::clone(&layout)));
        layout
    })
}

fn enumerate(monitors: &mut Vec<Monitor>) -> bool {
    unsafe extern "system" fn record(_: HMONITOR, _: HDC, bounds: *mut RECT, data: LPARAM) -> i32 {
        let monitors = unsafe { &mut *(data as *mut Vec<Monitor>) };
        let bounds = unsafe { &*bounds };
        monitors.push(Monitor {
            left: bounds.left,
            top: bounds.top,
            right: bounds.right,
            bottom: bounds.bottom,
        });
        1
    }
    unsafe {
        let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let result = EnumDisplayMonitors(
            null_mut(),
            null(),
            Some(record),
            monitors as *mut Vec<Monitor> as LPARAM,
        );
        if !previous.is_null() {
            SetThreadDpiAwarenessContext(previous);
        }
        result != 0
    }
}

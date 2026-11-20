use std::{
    cell::RefCell,
    io,
    mem::size_of,
    ptr::{null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering::Relaxed},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use tokio::sync::{mpsc, oneshot};
use windows_sys::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
        System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext},
            Input::{
                GetRawInputData, HRAWINPUT,
                KeyboardAndMouse::{GetAsyncKeyState, VK_F12},
                RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK, RIDEV_REMOVE,
                RIM_TYPEMOUSE, RegisterRawInputDevices,
            },
            WindowsAndMessaging::*,
        },
    },
    core::w,
};

use super::WindowsDesktop;
use crate::{
    layout::Layout,
    protocol::{Edge, Frame, Input, MAX_MOVEMENT},
    receiver::Desktop,
    session_health::QueuedFrame,
    state::{Control, Point, Screen},
};

const NO_RETURN: u32 = u32::MAX;
const COOLDOWN: Duration = Duration::from_millis(200);
const QUEUE_CAPACITY: usize = 1024;

thread_local! {
    static HOOK: RefCell<Option<HookState>> = const { RefCell::new(None) };
}

struct Link {
    alive: AtomicBool,
    forwarding: AtomicBool,
    return_at: AtomicU32,
    thread_id: AtomicU32,
    events: mpsc::Sender<QueuedFrame>,
}

pub struct Capture(Arc<Link>, Option<std::thread::JoinHandle<()>>);

impl Capture {
    pub async fn start(from: Edge, to: Edge) -> Result<(Self, mpsc::Receiver<QueuedFrame>)> {
        let (events, rx) = mpsc::channel(QUEUE_CAPACITY);
        let (ready, initialized) = oneshot::channel();
        let mut capture = Self(
            Arc::new(Link {
                alive: AtomicBool::new(true),
                forwarding: AtomicBool::new(false),
                return_at: AtomicU32::new(NO_RETURN),
                thread_id: AtomicU32::new(0),
                events,
            }),
            None,
        );
        let link = Arc::clone(&capture.0);
        capture.1 = Some(
            std::thread::Builder::new()
                .name("edgebridge-input".into())
                .spawn(move || run_hooks(link, from, to, ready))?,
        );
        initialized.await.context("Windows input thread exited")??;
        Ok((capture, rx))
    }

    pub fn alive(&self) -> bool {
        self.0.alive.load(Relaxed)
    }
    pub fn forwarding(&self) -> bool {
        self.0.forwarding.load(Relaxed)
    }
    pub fn return_to(&self, along: u16) {
        self.0.return_at.store(u32::from(along), Relaxed);
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.0.alive.store(false, Relaxed);
        let thread = self.0.thread_id.load(Relaxed);
        if thread != 0 {
            unsafe {
                PostThreadMessageW(thread, WM_QUIT, 0, 0);
            }
        }
        if let Some(join) = self.1.take() {
            let _ = join.join();
        }
    }
}

struct HookState {
    link: Arc<Link>,
    control: Control,
    from: Edge,
    to: Edge,
    last_local: Option<Point>,
    last_along: u16,
    layout: Arc<Layout>,
    cooldown_until: Instant,
    keys: [bool; 512],
    consume_key_up: [bool; 512],
    buttons: [bool; 5],
    consume_button_up: [bool; 5],
}

impl HookState {
    fn new(link: Arc<Link>, from: Edge, to: Edge) -> Self {
        let mut control = Control::Offline;
        control.connected();
        Self {
            link,
            control,
            from,
            to,
            last_local: cursor(),
            last_along: u16::MAX / 2,
            layout: super::layout(),
            cooldown_until: Instant::now(),
            keys: [false; 512],
            consume_key_up: [false; 512],
            buttons: [false; 5],
            consume_button_up: [false; 5],
        }
    }

    fn offer(&mut self, frame: Frame) -> bool {
        if self
            .link
            .events
            .try_send(QueuedFrame::new(frame, Instant::now()))
            .is_ok()
        {
            return true;
        }
        self.link.alive.store(false, Relaxed);
        false
    }

    fn remote(&self) -> bool {
        self.control == Control::Remote
    }

    fn refresh_layout(&mut self) {
        let layout = super::layout();
        if self.layout == layout {
            return;
        }
        self.layout = layout;
        if self.remote() {
            self.offer(Frame::Release);
            self.return_to(self.last_along);
        } else {
            self.last_local = None;
        }
    }

    fn return_to(&mut self, along: u16) {
        if !self.remote() {
            return;
        }
        self.control.release();
        self.link.forwarding.store(false, Relaxed);
        self.consume_key_up = self.keys;
        self.consume_button_up = self.buttons;
        self.cooldown_until = Instant::now() + COOLDOWN;
        let point = super::layout().entry(self.from, along);
        self.last_local = Some(point);
        let _ = WindowsDesktop.move_to(point);
    }

    fn mouse(&mut self, msg: u32, data: MSLLHOOKSTRUCT) -> bool {
        if data.flags & LLMHF_INJECTED != 0 {
            return false;
        }
        self.refresh_layout();
        match msg {
            WM_MOUSEMOVE => {
                if self.remote() {
                    return true;
                }
                let point = Point {
                    x: data.pt.x,
                    y: data.pt.y,
                };
                let last = self.last_local.replace(point);
                if Instant::now() < self.cooldown_until {
                    return false;
                }
                if let Some(last) = last
                    && self.layout.contains(last)
                    && !self.layout.at(self.from, last)
                    && self.layout.at(self.from, point)
                    && Screen::outward(self.from, point.x - last.x, point.y - last.y)
                {
                    let held = self.keys.contains(&true)
                        || self.buttons.contains(&true)
                        || (1..=254).any(|vk| unsafe { GetAsyncKeyState(vk) < 0 });
                    if self.control.enter(true, held) {
                        self.last_along = self.layout.along(self.from, point);
                        if self.offer(Frame::Enter(self.to, self.last_along)) {
                            self.link.forwarding.store(true, Relaxed);
                            return true;
                        }
                    }
                }
                false
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL if self.remote() => self.offer(Frame::Input(
                Input::Wheel((data.mouseData >> 16) as i16, msg == WM_MOUSEHWHEEL),
            )),
            _ => {
                let (button, down) = match msg {
                    WM_LBUTTONDOWN | WM_LBUTTONUP => (0, msg == WM_LBUTTONDOWN),
                    WM_RBUTTONDOWN | WM_RBUTTONUP => (1, msg == WM_RBUTTONDOWN),
                    WM_MBUTTONDOWN | WM_MBUTTONUP => (2, msg == WM_MBUTTONDOWN),
                    WM_XBUTTONDOWN | WM_XBUTTONUP => {
                        let id = if data.mouseData >> 16 == 1 { 3 } else { 4 };
                        (id, msg == WM_XBUTTONDOWN)
                    }
                    _ => return false,
                };
                self.buttons[button] = down;
                if self.consume_button_up[button] {
                    self.consume_button_up[button] = down;
                    return true;
                }
                self.remote() && self.offer(Frame::Input(Input::Button(button as u8, down)))
            }
        }
    }

    fn keyboard(&mut self, data: KBDLLHOOKSTRUCT) -> bool {
        if data.flags & LLKHF_INJECTED != 0 {
            return false;
        }
        self.refresh_layout();
        let Ok(scan) = u8::try_from(data.scanCode) else {
            return self.remote();
        };
        if scan == 0 {
            return self.remote();
        }
        let extended = data.flags & LLKHF_EXTENDED != 0;
        let down = data.flags & LLKHF_UP == 0;
        let id = scan as usize + usize::from(extended) * 256;
        self.keys[id] = down;
        if self.consume_key_up[id] {
            self.consume_key_up[id] = down;
            return true;
        }
        if !self.remote() {
            return false;
        }
        let ctrl = self.keys[0x1d] || self.keys[256 + 0x1d];
        let alt = self.keys[0x38] || self.keys[256 + 0x38];
        if down && data.vkCode == u32::from(VK_F12) && ctrl && alt {
            self.offer(Frame::Release);
            self.return_to(self.last_along);
            return true;
        }
        self.offer(Frame::Input(Input::Key {
            scan,
            extended,
            down,
        }))
    }

    fn raw_motion(&mut self, dx: i32, dy: i32) {
        self.refresh_layout();
        if !self.remote() || (dx == 0 && dy == 0) {
            return;
        }
        let limit = i32::from(MAX_MOVEMENT);
        self.offer(Frame::Input(Input::Move(
            dx.clamp(-limit, limit) as i16,
            dy.clamp(-limit, limit) as i16,
        )));
    }
}

fn cursor() -> Option<Point> {
    let mut point = POINT::default();
    (unsafe { GetCursorPos(&mut point) } != 0).then_some(Point {
        x: point.x,
        y: point.y,
    })
}

fn handle(f: impl FnOnce(&mut HookState) -> bool) -> bool {
    HOOK.with(|state| {
        let Ok(mut state) = state.try_borrow_mut() else {
            return false;
        };
        state
            .as_mut()
            .is_some_and(|state| state.link.alive.load(Relaxed) && f(state))
    })
}

unsafe extern "system" fn mouse_hook(code: i32, message: WPARAM, data: LPARAM) -> LRESULT {
    if code >= 0
        && handle(|state| state.mouse(message as u32, unsafe { *(data as *const MSLLHOOKSTRUCT) }))
    {
        return 1;
    }
    unsafe { CallNextHookEx(null_mut(), code, message, data) }
}

unsafe extern "system" fn keyboard_hook(code: i32, message: WPARAM, data: LPARAM) -> LRESULT {
    if code >= 0 && handle(|state| state.keyboard(unsafe { *(data as *const KBDLLHOOKSTRUCT) })) {
        return 1;
    }
    unsafe { CallNextHookEx(null_mut(), code, message, data) }
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if message == WM_INPUT {
        let mut raw = RAWINPUT::default();
        let mut size = size_of::<RAWINPUT>() as u32;
        let read = unsafe {
            GetRawInputData(
                l as HRAWINPUT,
                RID_INPUT,
                (&mut raw as *mut RAWINPUT).cast(),
                &mut size,
                size_of::<RAWINPUTHEADER>() as u32,
            )
        };
        if read == size_of::<RAWINPUT>() as u32 && raw.header.dwType == RIM_TYPEMOUSE {
            let mouse = unsafe { raw.data.mouse };
            if mouse.usFlags & 1 == 0 {
                handle(|state| {
                    state.raw_motion(mouse.lLastX, mouse.lLastY);
                    false
                });
            }
        }
    }
    unsafe { DefWindowProcW(window, message, w, l) }
}

#[derive(Default)]
struct WindowsHooks {
    mouse: HHOOK,
    keyboard: HHOOK,
    window: HWND,
    instance: windows_sys::Win32::Foundation::HINSTANCE,
    class_registered: bool,
    raw_registered: bool,
    timer: usize,
}

const CLASS: windows_sys::core::PCWSTR = w!("EdgeBridgeInput");

impl WindowsHooks {
    fn install() -> Result<Self> {
        let mut hooks = Self::default();
        unsafe {
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            hooks.instance = GetModuleHandleW(null());
            let wc = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: hooks.instance,
                lpszClassName: CLASS,
                ..Default::default()
            };
            if RegisterClassW(&wc) == 0 {
                return Err(io::Error::last_os_error()).context("Register input window");
            }
            hooks.class_registered = true;
            hooks.window = CreateWindowExW(
                0,
                CLASS,
                CLASS,
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                null_mut(),
                hooks.instance,
                null(),
            );
            if hooks.window.is_null() {
                return Err(io::Error::last_os_error()).context("Create input window");
            }
            raw_registration(RIDEV_INPUTSINK, hooks.window)?;
            hooks.raw_registered = true;
            hooks.mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), hooks.instance, 0);
            if hooks.mouse.is_null() {
                return Err(io::Error::last_os_error()).context("Install mouse hook");
            }
            hooks.keyboard =
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), hooks.instance, 0);
            if hooks.keyboard.is_null() {
                return Err(io::Error::last_os_error()).context("Install keyboard hook");
            }
            hooks.timer = SetTimer(null_mut(), 0, 15, None);
            if hooks.timer == 0 {
                return Err(io::Error::last_os_error()).context("Start input timer");
            }
        }
        Ok(hooks)
    }
}

fn raw_registration(flags: u32, target: HWND) -> Result<()> {
    let raw = RAWINPUTDEVICE {
        usUsagePage: 1,
        usUsage: 2,
        dwFlags: flags,
        hwndTarget: target,
    };
    if unsafe { RegisterRawInputDevices(&raw, 1, size_of::<RAWINPUTDEVICE>() as u32) } == 0 {
        return Err(io::Error::last_os_error()).context("Register raw mouse input");
    }
    Ok(())
}

impl Drop for WindowsHooks {
    fn drop(&mut self) {
        unsafe {
            if self.timer != 0 {
                KillTimer(null_mut(), self.timer);
            }
            if !self.keyboard.is_null() {
                UnhookWindowsHookEx(self.keyboard);
            }
            if !self.mouse.is_null() {
                UnhookWindowsHookEx(self.mouse);
            }
            if self.raw_registered {
                let _ = raw_registration(RIDEV_REMOVE, null_mut());
            }
            if !self.window.is_null() {
                DestroyWindow(self.window);
            }
            if self.class_registered {
                UnregisterClassW(CLASS, self.instance);
            }
        }
    }
}

fn run_hooks(link: Arc<Link>, from: Edge, to: Edge, ready: oneshot::Sender<Result<()>>) {
    let hooks = match WindowsHooks::install() {
        Ok(hooks) => hooks,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    HOOK.with(|state| *state.borrow_mut() = Some(HookState::new(Arc::clone(&link), from, to)));
    link.thread_id
        .store(unsafe { GetCurrentThreadId() }, Relaxed);
    if ready.send(Ok(())).is_err() {
        return;
    }
    let mut msg = MSG::default();
    while link.alive.load(Relaxed) && unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) } > 0 {
        if msg.hwnd.is_null() && msg.message == WM_TIMER && msg.wParam == hooks.timer {
            let along = link.return_at.swap(NO_RETURN, Relaxed);
            if along != NO_RETURN {
                handle(|state| {
                    state.return_to(along as u16);
                    false
                });
            }
        } else {
            unsafe {
                DispatchMessageW(&msg);
            }
        }
    }
    link.alive.store(false, Relaxed);
    drop(hooks);
    HOOK.with(|state| {
        if let Some(mut state) = state.borrow_mut().take() {
            state.return_to(state.last_along);
            state.control.disconnected();
        }
    });
}

use std::{io, mem::size_of, sync::Arc};

use anyhow::{Result, bail};
use windows_sys::Win32::UI::{
    HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext},
    Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
        KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN,
        MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN,
        MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT,
        SendInput,
    },
    WindowsAndMessaging::SetCursorPos,
};

use crate::{layout::Layout, protocol::Input, receiver::Desktop, state::Point};

pub struct WindowsDesktop;

impl Desktop for WindowsDesktop {
    fn layout(&self) -> Arc<Layout> {
        super::layout()
    }

    fn move_to(&self, point: Point) -> Result<()> {
        unsafe {
            let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let result = SetCursorPos(point.x, point.y);
            let error = (result == 0).then(io::Error::last_os_error);
            if !previous.is_null() {
                SetThreadDpiAwarenessContext(previous);
            }
            match error {
                Some(error) => Err(error.into()),
                None => Ok(()),
            }
        }
    }

    fn input(&self, event: Input) -> Result<()> {
        let input = match event {
            Input::Key {
                scan,
                extended,
                down,
            } => INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wScan: u16::from(scan),
                        dwFlags: KEYEVENTF_SCANCODE
                            | if extended { KEYEVENTF_EXTENDEDKEY } else { 0 }
                            | if down { 0 } else { KEYEVENTF_KEYUP },
                        ..Default::default()
                    },
                },
            },
            Input::Button(button, down) => {
                let flags = [
                    (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
                    (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
                    (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
                    (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP),
                    (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP),
                ];
                let Some(&(press, release)) = flags.get(button as usize) else {
                    bail!("Invalid mouse button")
                };
                mouse(
                    if down { press } else { release },
                    u32::from(button.saturating_sub(2)),
                )
            }
            Input::Wheel(delta, horizontal) => mouse(
                if horizontal {
                    MOUSEEVENTF_HWHEEL
                } else {
                    MOUSEEVENTF_WHEEL
                },
                i32::from(delta) as u32,
            ),
            Input::Move(..) => bail!("Movement requires an absolute target"),
        };
        if unsafe { SendInput(1, &input, size_of::<INPUT>() as i32) } != 1 {
            bail!(
                "Windows rejected remote input. Check whether the active app is elevated or the desktop is locked."
            );
        }
        Ok(())
    }
}

fn mouse(flags: u32, data: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dwFlags: flags,
                mouseData: data,
                ..Default::default()
            },
        },
    }
}

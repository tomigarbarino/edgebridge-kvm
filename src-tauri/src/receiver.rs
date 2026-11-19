use std::collections::BTreeSet;

use anyhow::Result;

use crate::{
    protocol::{Edge, Frame, Input},
    state::{Point, Screen},
};

pub trait Desktop {
    fn screen(&self) -> Screen;
    fn move_to(&self, point: Point) -> Result<()>;
    fn input(&self, event: Input) -> Result<()>;
}

pub struct Receiver<D: Desktop> {
    desktop: D,
    active: Option<(Edge, Point)>,
    interior: bool,
    keys: BTreeSet<(u8, bool)>,
    buttons: BTreeSet<u8>,
}

impl<D: Desktop> Receiver<D> {
    pub fn new(desktop: D) -> Self {
        Self {
            desktop,
            active: None,
            interior: false,
            keys: BTreeSet::new(),
            buttons: BTreeSet::new(),
        }
    }

    pub fn enter(&mut self, edge: Edge, position: u16) -> Result<()> {
        self.release();
        let point = self.desktop.screen().entry(edge, position);
        self.desktop.move_to(point)?;
        self.active = Some((edge, point));
        Ok(())
    }

    pub fn input(&mut self, event: Input) -> Result<Option<Frame>> {
        let Some((edge, current)) = self.active else {
            return Ok(None);
        };
        if let Input::Move(dx, dy) = event {
            let screen = self.desktop.screen();
            let (dx, dy) = (i32::from(dx), i32::from(dy));
            let next = screen.clamp(Point {
                x: current.x + dx,
                y: current.y + dy,
            });
            self.desktop.move_to(next)?;
            if !screen.at(edge, next) && !Screen::outward(edge, dx, dy) && next != current {
                self.interior = true;
            }
            if self.interior
                && screen.at(edge, next)
                && Screen::outward(edge, dx, dy)
                && self.keys.is_empty()
                && self.buttons.is_empty()
            {
                self.release();
                return Ok(Some(Frame::Return(screen.along(edge, next))));
            }
            self.active = Some((edge, next));
            return Ok(None);
        }

        self.desktop.input(event)?;
        match event {
            Input::Button(button, down) => {
                if down {
                    self.buttons.insert(button);
                } else {
                    self.buttons.remove(&button);
                }
            }
            Input::Key {
                scan,
                extended,
                down,
            } => {
                if down {
                    self.keys.insert((scan, extended));
                } else {
                    self.keys.remove(&(scan, extended));
                }
            }
            _ => {}
        }
        Ok(None)
    }

    pub fn release(&mut self) {
        self.active = None;
        self.interior = false;
        for (scan, extended) in std::mem::take(&mut self.keys) {
            let _ = self.desktop.input(Input::Key {
                scan,
                extended,
                down: false,
            });
        }
        for button in std::mem::take(&mut self.buttons) {
            let _ = self.desktop.input(Input::Button(button, false));
        }
    }
}

impl<D: Desktop> Drop for Receiver<D> {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    #[derive(Clone, Default)]
    struct Mock(Rc<RefCell<Vec<Input>>>);

    impl Desktop for Mock {
        fn screen(&self) -> Screen {
            Screen {
                width: 1000,
                height: 700,
            }
        }
        fn move_to(&self, _: Point) -> Result<()> {
            Ok(())
        }
        fn input(&self, event: Input) -> Result<()> {
            self.0.borrow_mut().push(event);
            Ok(())
        }
    }

    #[test]
    fn outward_entry_momentum_does_not_bounce_until_inward_motion() -> Result<()> {
        let mut receiver = Receiver::new(Mock::default());
        receiver.enter(Edge::Right, 32000)?;
        assert_eq!(receiver.input(Input::Move(10, 0))?, None);
        assert_eq!(receiver.input(Input::Move(-100, 0))?, None);
        assert!(matches!(
            receiver.input(Input::Move(100, 0))?,
            Some(Frame::Return(_))
        ));
        assert_eq!(receiver.input(Input::Move(10, 0))?, None);
        Ok(())
    }

    #[test]
    fn drops_held_buttons_and_keys_and_ignores_inactive_input() -> Result<()> {
        let mock = Mock::default();
        let mut receiver = Receiver::new(mock.clone());
        receiver.input(Input::Button(0, true))?;
        receiver.enter(Edge::Right, 0)?;
        let key = |down| Input::Key {
            scan: 30,
            extended: true,
            down,
        };
        receiver.input(key(true))?;
        receiver.input(Input::Button(0, true))?;
        receiver.input(Input::Move(-80, 0))?;
        assert_eq!(receiver.input(Input::Move(80, 0))?, None);
        drop(receiver);
        assert_eq!(
            *mock.0.borrow(),
            [
                key(true),
                Input::Button(0, true),
                key(false),
                Input::Button(0, false)
            ]
        );
        Ok(())
    }
}

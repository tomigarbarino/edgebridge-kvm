use std::{collections::BTreeSet, sync::Arc};

use anyhow::Result;

use crate::{
    layout::Layout,
    protocol::{Edge, Frame, Input},
    state::{Point, Screen},
};

pub trait Desktop {
    fn layout(&self) -> Arc<Layout>;
    fn move_to(&self, point: Point) -> Result<()>;
    fn input(&self, event: Input) -> Result<()>;
}

pub struct Receiver<D: Desktop> {
    desktop: D,
    active: Option<(Edge, Point, Arc<Layout>)>,
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
        let layout = self.desktop.layout();
        let point = layout.entry(edge, position);
        self.desktop.move_to(point)?;
        self.active = Some((edge, point, layout));
        Ok(())
    }

    pub fn input(&mut self, event: Input) -> Result<Option<Frame>> {
        let Some((edge, current, previous)) = &self.active else {
            return Ok(None);
        };
        let (edge, current) = (*edge, *current);
        let layout = self.desktop.layout();
        if *previous != layout {
            let along = previous.along(edge, current);
            self.release();
            return Ok(Some(Frame::Return(along)));
        }
        if let Input::Move(dx, dy) = event {
            let (dx, dy) = (i32::from(dx), i32::from(dy));
            let next = layout.clamp(Point {
                x: current.x + dx,
                y: current.y + dy,
            });
            self.desktop.move_to(next)?;
            if !layout.at(edge, next) && !Screen::outward(edge, dx, dy) && next != current {
                self.interior = true;
            }
            if self.interior
                && layout.at(edge, next)
                && Screen::outward(edge, dx, dy)
                && self.keys.is_empty()
                && self.buttons.is_empty()
            {
                self.release();
                return Ok(Some(Frame::Return(layout.along(edge, next))));
            }
            self.active = Some((edge, next, layout));
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
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    use super::*;

    #[derive(Clone, Default)]
    struct Mock(Rc<RefCell<Vec<Input>>>, Rc<Cell<bool>>);

    impl Desktop for Mock {
        fn layout(&self) -> Arc<Layout> {
            Arc::new(Layout::single(Screen {
                width: if self.1.get() { 1200 } else { 1000 },
                height: 700,
            }))
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
    fn display_change_returns_control_and_releases_held_input() -> Result<()> {
        let desktop = Mock::default();
        let mut receiver = Receiver::new(desktop.clone());
        receiver.enter(Edge::Right, 32768)?;
        receiver.input(Input::Button(0, true))?;
        desktop.1.set(true);
        assert!(matches!(
            receiver.input(Input::Move(10, 0))?,
            Some(Frame::Return(_))
        ));
        assert_eq!(
            *desktop.0.borrow(),
            [Input::Button(0, true), Input::Button(0, false)]
        );
        Ok(())
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

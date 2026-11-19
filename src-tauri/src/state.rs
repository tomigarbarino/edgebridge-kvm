use crate::protocol::Edge;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Offline,
    Local,
    Remote,
}

impl Control {
    pub fn connected(&mut self) {
        *self = Self::Local;
    }

    pub fn enter(&mut self, at_edge: bool, held: bool) -> bool {
        if *self != Self::Local || !at_edge || held {
            return false;
        }
        *self = Self::Remote;
        true
    }

    pub fn release(&mut self) {
        if *self == Self::Remote {
            *self = Self::Local;
        }
    }

    pub fn disconnected(&mut self) {
        *self = Self::Offline;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct Screen {
    pub width: i32,
    pub height: i32,
}

impl Screen {
    pub fn clamp(self, p: Point) -> Point {
        Point {
            x: p.x.clamp(0, self.width - 1),
            y: p.y.clamp(0, self.height - 1),
        }
    }

    pub fn at(self, edge: Edge, point: Point) -> bool {
        match edge {
            Edge::Left => point.x <= 0,
            Edge::Right => point.x >= self.width - 1,
            Edge::Top => point.y <= 0,
            Edge::Bottom => point.y >= self.height - 1,
        }
    }

    pub fn outward(edge: Edge, dx: i32, dy: i32) -> bool {
        match edge {
            Edge::Left => dx < 0,
            Edge::Right => dx > 0,
            Edge::Top => dy < 0,
            Edge::Bottom => dy > 0,
        }
    }

    pub fn along(self, edge: Edge, p: Point) -> u16 {
        let (position, length) = match edge {
            Edge::Left | Edge::Right => (p.y, self.height),
            Edge::Top | Edge::Bottom => (p.x, self.width),
        };
        (i64::from(position.clamp(0, length - 1)) * i64::from(u16::MAX)
            / i64::from((length - 1).max(1))) as u16
    }

    pub fn entry(self, edge: Edge, along: u16) -> Point {
        let map = |extent| (i64::from(along) * i64::from(extent - 1) / i64::from(u16::MAX)) as i32;
        let point = match edge {
            Edge::Left => Point {
                x: 2,
                y: map(self.height),
            },
            Edge::Right => Point {
                x: self.width - 3,
                y: map(self.height),
            },
            Edge::Top => Point {
                x: map(self.width),
                y: 2,
            },
            Edge::Bottom => Point {
                x: map(self.width),
                y: self.height - 3,
            },
        };
        self.clamp(point)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_local_control_and_never_enters_offline_or_with_held_input() {
        let mut state = Control::Offline;
        assert!(!state.enter(true, false));
        state.connected();
        assert!(!state.enter(false, false));
        assert!(!state.enter(true, true));
        assert!(state.enter(true, false));
        assert!(!state.enter(true, false));
        state.release();
        assert_eq!(state, Control::Local);
        assert!(state.enter(true, false));
        state.disconnected();
        assert_eq!(state, Control::Offline);
    }

    #[test]
    fn defaults_cross_left_to_right_and_return_at_scaled_height() {
        let primary = Screen {
            width: 2560,
            height: 1440,
        };
        let secondary = Screen {
            width: 1920,
            height: 1080,
        };
        let exit = Point { x: 0, y: 720 };
        assert!(primary.at(Edge::Left, exit));
        assert!(!primary.at(Edge::Right, exit));
        let entry = secondary.entry(Edge::Right, primary.along(Edge::Left, exit));
        assert_eq!(entry.x, 1917);
        assert!((entry.y - 540).abs() <= 1);
        let return_point = secondary.clamp(Point {
            x: 1922,
            y: entry.y,
        });
        assert!(secondary.at(Edge::Right, return_point));
        let back = primary.entry(Edge::Left, secondary.along(Edge::Right, return_point));
        assert_eq!(back.x, 2);
        assert!((back.y - exit.y).abs() <= 3);
    }

    #[test]
    fn vertical_edges_and_corners_map_within_the_display() {
        let narrow = Screen {
            width: 600,
            height: 1100,
        };
        let wide = Screen {
            width: 1900,
            height: 900,
        };
        let edge = Edge::Top;
        let p = Point { x: 599, y: 0 };
        assert!(narrow.at(edge, p));
        assert!(Screen::outward(edge, 0, -5));
        assert_eq!(
            wide.entry(Edge::Bottom, narrow.along(edge, p)),
            Point { x: 1899, y: 897 }
        );
    }
}

use crate::{
    protocol::Edge,
    state::{Point, Screen},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Monitor {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Monitor {
    fn closest_point(self, point: Point) -> Point {
        Point {
            x: point.x.clamp(self.left, self.right - 1),
            y: point.y.clamp(self.top, self.bottom - 1),
        }
    }

    fn limits(self, edge: Edge) -> (i32, i32, i32) {
        match edge {
            Edge::Left => (self.left, self.top, self.bottom - 1),
            Edge::Right => (self.right - 1, self.top, self.bottom - 1),
            Edge::Top => (self.top, self.left, self.right - 1),
            Edge::Bottom => (self.bottom - 1, self.left, self.right - 1),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Layout(Vec<Monitor>);

impl Layout {
    pub fn new(mut monitors: Vec<Monitor>) -> Option<Self> {
        monitors.retain(|monitor| monitor.left < monitor.right && monitor.top < monitor.bottom);
        monitors.sort_unstable();
        monitors.dedup();
        (!monitors.is_empty()).then_some(Self(monitors))
    }

    pub fn single(screen: Screen) -> Self {
        Self(vec![Monitor {
            left: 0,
            top: 0,
            right: screen.width.max(1),
            bottom: screen.height.max(1),
        }])
    }

    pub fn contains(&self, point: Point) -> bool {
        self.0
            .iter()
            .any(|monitor| monitor.closest_point(point) == point)
    }

    pub fn clamp(&self, point: Point) -> Point {
        self.0
            .iter()
            .map(|monitor| monitor.closest_point(point))
            .min_by_key(|p| {
                let dx = i64::from(p.x) - i64::from(point.x);
                let dy = i64::from(p.y) - i64::from(point.y);
                dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
            })
            .unwrap_or(point)
    }

    pub fn at(&self, edge: Edge, point: Point) -> bool {
        let (axis, cross) = axes(edge, point);
        self.0
            .iter()
            .map(|monitor| monitor.limits(edge))
            .filter(|&(_, start, end)| (start..=end).contains(&cross))
            .map(|(boundary, _, _)| outward_coordinate(edge, boundary))
            .max()
            .is_some_and(|boundary| outward_coordinate(edge, axis) >= boundary)
    }

    fn range(&self, edge: Edge) -> (i32, i32) {
        self.0
            .iter()
            .map(|monitor| monitor.limits(edge))
            .fold((i32::MAX, i32::MIN), |(min, max), (_, start, end)| {
                (min.min(start), max.max(end))
            })
    }

    pub fn along(&self, edge: Edge, point: Point) -> u16 {
        let (_, cross) = axes(edge, point);
        let (min, max) = self.range(edge);
        let offset = i64::from(cross.clamp(min, max)) - i64::from(min);
        (offset * i64::from(u16::MAX) / (i64::from(max) - i64::from(min)).max(1)) as u16
    }

    pub fn entry(&self, edge: Edge, along: u16) -> Point {
        let (min, max) = self.range(edge);
        let cross = (i64::from(min)
            + i64::from(along) * (i64::from(max) - i64::from(min)) / i64::from(u16::MAX))
            as i32;
        let monitor = self
            .0
            .iter()
            .min_by_key(|monitor| {
                let (boundary, start, end) = monitor.limits(edge);
                (
                    cross.abs_diff(cross.clamp(start, end)),
                    -outward_coordinate(edge, boundary),
                )
            })
            .expect("a desktop layout always has a monitor");
        let (boundary, start, end) = monitor.limits(edge);
        let axis = match edge {
            Edge::Left | Edge::Top => boundary.saturating_add(2),
            Edge::Right | Edge::Bottom => boundary.saturating_sub(2),
        };
        let cross = cross.clamp(start, end);
        monitor.closest_point(match edge {
            Edge::Left | Edge::Right => Point { x: axis, y: cross },
            Edge::Top | Edge::Bottom => Point { x: cross, y: axis },
        })
    }
}

fn axes(edge: Edge, point: Point) -> (i32, i32) {
    match edge {
        Edge::Left | Edge::Right => (point.x, point.y),
        Edge::Top | Edge::Bottom => (point.y, point.x),
    }
}

fn outward_coordinate(edge: Edge, value: i32) -> i64 {
    match edge {
        Edge::Right | Edge::Bottom => i64::from(value),
        Edge::Left | Edge::Top => -i64::from(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_the_middle_monitor_moves_the_handoff_to_the_new_outer_edge() {
        let internal = Monitor {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let middle_to_left = Monitor {
            left: -2560,
            top: -100,
            right: 0,
            bottom: 1340,
        };
        let single = Layout::new(vec![internal]).unwrap();
        let extended = Layout::new(vec![internal, middle_to_left]).unwrap();
        let seam = Point { x: 0, y: 540 };
        assert!(single.at(Edge::Left, seam));
        assert!(!extended.at(Edge::Left, seam));
        assert!(extended.at(Edge::Left, Point { x: -2560, y: 540 }));
        assert_eq!(extended.entry(Edge::Left, 32768).x, -2558);
        assert_eq!(single.entry(Edge::Left, 32768).x, 2);
    }

    #[test]
    fn staggered_displays_use_the_nearest_real_screen() {
        let layout = Layout::new(vec![
            Monitor {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 800,
            },
            Monitor {
                left: 1920,
                top: 500,
                right: 3200,
                bottom: 1524,
            },
        ])
        .unwrap();
        assert!(layout.at(Edge::Right, Point { x: 1919, y: 400 }));
        assert!(!layout.at(Edge::Right, Point { x: 1919, y: 600 }));
        let gap = Point { x: 2050, y: 250 };
        assert!(!layout.contains(gap));
        assert!(layout.contains(layout.clamp(gap)));
        assert!(layout.contains(layout.entry(Edge::Right, 0)));
    }
}

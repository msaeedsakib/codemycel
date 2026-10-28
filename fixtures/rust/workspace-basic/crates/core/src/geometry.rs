use std::f64::consts::PI;
use std::fmt;

/// A point on the plane.
#[derive(Debug, Clone)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

pub struct Circle {
    pub center: Point,
    pub(crate) radius: f64,
}

pub enum Shape {
    Round(Circle),
    Square { side: f64 },
    Empty,
}

pub trait Area {
    fn area(&self) -> f64;

    fn name(&self) -> String;

    fn describe(&self) -> String {
        format!("{} of area {}", self.name(), self.area())
    }
}

impl fmt::Display for Circle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "circle at {}, {}", self.center.x, self.center.y)
    }
}

impl fmt::Debug for Circle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Circle").field("radius", &self.radius).finish()
    }
}

impl Area for Circle {
    fn area(&self) -> f64 {
        PI * self.radius * self.radius
    }

    fn name(&self) -> String {
        String::from("circle")
    }
}

pub fn sides(shape: &Shape) -> u32 {
    match shape {
        Shape::Round(_) => 0,
        Shape::Square { .. } => 4,
        Shape::Empty => 0,
    }
}

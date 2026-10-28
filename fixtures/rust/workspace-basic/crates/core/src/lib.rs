//! Shapes, their areas, and how they render.

pub mod geometry;
pub mod render;
pub mod store;
mod circle;

pub use geometry::{Area, Circle, Point, Shape};
pub use render::Render;

pub const MAX_SHAPES: usize = 16;

pub static GREETING: &str = "shapes";

/// Sums the area of every shape behind a trait object.
pub fn total_area(shapes: &[Box<dyn Area>]) -> f64 {
    shapes.iter().map(|shape| shape.area()).sum()
}

pub struct Wrapper<T> {
    pub inner: T,
}

impl<T: Clone> Wrapper<T> {
    pub fn new(inner: T) -> Self {
        Wrapper { inner }
    }

    pub fn get(&self) -> T {
        self.inner.clone()
    }
}

pub fn parse_all<T: std::str::FromStr>(items: &[&str]) -> Vec<T> {
    items.iter().filter_map(|item| item.parse::<T>().ok()).collect()
}

pub fn zeros<const N: usize>() -> [u8; N] {
    [0; N]
}

use crate::geometry::{Circle, Point};

impl Circle {
    pub fn new(center: Point, radius: f64) -> Self {
        Circle { center, radius }
    }

    pub fn scaled(&self, factor: f64) -> Circle {
        let radius = self.radius * factor;
        Self::new(self.center.clone(), radius)
    }
}

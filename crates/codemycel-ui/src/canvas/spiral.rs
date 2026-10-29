use super::Point;

const GOLDEN_ANGLE: f64 = 2.399_963_229_728_653;

/// `count` offsets from a centre on a sunflower spiral, `spacing` apart on average, nearest
/// first. Used to place the members of an expanded community inside its circle.
pub fn spiral(count: usize, spacing: f64) -> Vec<Point> {
    (0..count)
        .map(|index| {
            let distance = spacing * (index as f64 + 0.5).sqrt();
            let angle = index as f64 * GOLDEN_ANGLE;
            Point::new(distance * angle.cos(), distance * angle.sin())
        })
        .collect()
}

/// The radius of the circle that holds `spiral(count, spacing)` with `margin` to spare.
pub fn spiral_radius(count: usize, spacing: f64, margin: f64) -> f64 {
    spacing * (count.max(1) as f64 - 0.5).sqrt() + margin
}

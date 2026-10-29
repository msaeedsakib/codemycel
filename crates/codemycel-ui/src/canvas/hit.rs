use super::Point;

/// The smallest circle a pointer can hit, so tiny nodes stay clickable.
const MIN_HIT_RADIUS: f64 = 6.0;

/// The index of the circle nearest `at` among those whose radius reaches it.
pub fn hit_test(positions: &[Point], radii: &[f64], at: Point) -> Option<usize> {
    positions
        .iter()
        .zip(radii)
        .enumerate()
        .filter_map(|(index, (point, radius))| {
            let distance = point.distance(at);
            (distance <= radius.max(MIN_HIT_RADIUS)).then_some((index, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

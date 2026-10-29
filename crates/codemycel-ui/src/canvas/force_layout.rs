#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn distance(self, other: Point) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

/// The area a layout fills: every node's circle stays `padding` inside `width` × `height`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub width: f64,
    pub height: f64,
    pub padding: f64,
}

impl Viewport {
    pub fn new(width: f64, height: f64, padding: f64) -> Self {
        Self {
            width,
            height,
            padding,
        }
    }

    fn center(self) -> Point {
        Point::new(self.width / 2.0, self.height / 2.0)
    }

    fn clamp(self, point: Point, radius: f64) -> Point {
        let clamp_axis = |value: f64, extent: f64| {
            let low = self.padding + radius;
            let high = extent - self.padding - radius;
            if low >= high {
                extent / 2.0
            } else {
                value.clamp(low, high)
            }
        };
        Point::new(
            clamp_axis(point.x, self.width),
            clamp_axis(point.y, self.height),
        )
    }
}

/// An undirected edge between two node indices; heavier links pull harder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    pub source: usize,
    pub target: usize,
    pub weight: usize,
}

const MAX_ITERATIONS: usize = 600;
const SETTLED: f64 = 0.05;
const GAP: f64 = 6.0;
const GRAVITY: f64 = 0.04;
const MAX_ZOOM: f64 = 3.0;

/// Positions for circles of `radii` joined by `links`, inside `viewport`. A force simulation
/// runs from seeded starting positions until it settles, then the result is fitted to the
/// viewport. The same inputs and `seed` always give the same positions.
pub fn force_layout(radii: &[f64], links: &[Link], viewport: Viewport, seed: u64) -> Vec<Point> {
    let count = radii.len();
    if count == 0 {
        return Vec::new();
    }
    let center = viewport.center();
    let inner_width = (viewport.width - 2.0 * viewport.padding).max(1.0);
    let inner_height = (viewport.height - 2.0 * viewport.padding).max(1.0);
    let ideal = (inner_width * inner_height / count as f64).sqrt() * 0.5;
    let heaviest = links
        .iter()
        .map(|link| link.weight)
        .max()
        .unwrap_or(1)
        .max(1) as f64;
    let links: Vec<(usize, usize, f64)> = links
        .iter()
        .filter(|link| link.source < count && link.target < count && link.source != link.target)
        .map(|link| {
            let strength = (1.0 + link.weight as f64).ln() / (1.0 + heaviest).ln();
            (link.source, link.target, 0.25 + 0.75 * strength)
        })
        .collect();

    let mut random = SplitMix(seed);
    let spread = inner_width.min(inner_height) / 3.0;
    let mut positions: Vec<Point> = radii
        .iter()
        .map(|&radius| {
            let angle = random.next_unit() * std::f64::consts::TAU;
            let distance = random.next_unit().sqrt() * spread;
            viewport.clamp(
                Point::new(
                    center.x + distance * angle.cos(),
                    center.y + distance * angle.sin(),
                ),
                radius,
            )
        })
        .collect();

    let mut temperature = inner_width.max(inner_height) / 8.0;
    let cooling = (0.01_f64).powf(1.0 / MAX_ITERATIONS as f64);
    let mut forces = vec![Point::default(); count];
    for _ in 0..MAX_ITERATIONS {
        forces
            .iter_mut()
            .for_each(|force| *force = Point::default());
        for a in 0..count {
            for b in a + 1..count {
                let dx = positions[a].x - positions[b].x;
                let dy = positions[a].y - positions[b].y;
                let distance = dx.hypot(dy).max(0.01);
                let clearance = (distance - radii[a] - radii[b]).max(0.5);
                let mut push = ideal * ideal / clearance / distance;
                if clearance < GAP {
                    push += (GAP - clearance) * 2.0 / distance;
                }
                let (fx, fy) = if distance > 0.01 {
                    (dx * push, dy * push)
                } else {
                    (push, 0.0)
                };
                forces[a].x += fx;
                forces[a].y += fy;
                forces[b].x -= fx;
                forces[b].y -= fy;
            }
        }
        for &(a, b, strength) in &links {
            let dx = positions[a].x - positions[b].x;
            let dy = positions[a].y - positions[b].y;
            let distance = dx.hypot(dy).max(0.01);
            let rest = radii[a] + radii[b];
            let stretch = (distance - rest).max(0.0);
            let pull = stretch * stretch / ideal * strength / distance;
            forces[a].x -= dx * pull;
            forces[a].y -= dy * pull;
            forces[b].x += dx * pull;
            forces[b].y += dy * pull;
        }
        let mut largest = 0.0_f64;
        for (index, position) in positions.iter_mut().enumerate() {
            let force = &mut forces[index];
            force.x -= (position.x - center.x) * GRAVITY * inner_height / inner_width;
            force.y -= (position.y - center.y) * GRAVITY * inner_width / inner_height;
            let magnitude = force.x.hypot(force.y);
            if magnitude <= f64::EPSILON {
                continue;
            }
            let step = magnitude.min(temperature);
            let moved = viewport.clamp(
                Point::new(
                    position.x + force.x / magnitude * step,
                    position.y + force.y / magnitude * step,
                ),
                radii[index],
            );
            largest = largest.max(moved.distance(*position));
            *position = moved;
        }
        temperature *= cooling;
        if largest < SETTLED {
            break;
        }
    }
    fit(&mut positions, radii, viewport);
    positions
}

/// Scales and centres `positions` to use the viewport, then clamps each circle inside it.
fn fit(positions: &mut [Point], radii: &[f64], viewport: Viewport) {
    let largest = radii.iter().copied().fold(0.0_f64, f64::max);
    let (mut min, mut max) = (
        Point::new(f64::INFINITY, f64::INFINITY),
        Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    for point in positions.iter() {
        min = Point::new(min.x.min(point.x), min.y.min(point.y));
        max = Point::new(max.x.max(point.x), max.y.max(point.y));
    }
    let available_x = viewport.width - 2.0 * (viewport.padding + largest);
    let available_y = viewport.height - 2.0 * (viewport.padding + largest);
    let span_x = max.x - min.x;
    let span_y = max.y - min.y;
    let scale = [(available_x, span_x), (available_y, span_y)]
        .iter()
        .filter(|(_, span)| *span > 1.0)
        .map(|(available, span)| available / span)
        .fold(MAX_ZOOM, f64::min)
        .max(0.0);
    let middle = Point::new((min.x + max.x) / 2.0, (min.y + max.y) / 2.0);
    let center = viewport.center();
    for (point, &radius) in positions.iter_mut().zip(radii) {
        *point = viewport.clamp(
            Point::new(
                center.x + (point.x - middle.x) * scale,
                center.y + (point.y - middle.y) * scale,
            ),
            radius,
        );
    }
}

/// A small deterministic generator; the layout only needs reproducible spread, not quality.
struct SplitMix(u64);

impl SplitMix {
    fn next_unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }
}

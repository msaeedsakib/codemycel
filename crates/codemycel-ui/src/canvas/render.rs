use std::f64::consts::TAU;

use wasm_bindgen::{JsCast, JsValue};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use super::Point;

/// One circle on the canvas, in layout coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub at: Point,
    pub radius: f64,
    /// Index into the theme's categorical palette.
    pub color: usize,
    pub label: String,
    /// The hover text's second line.
    pub detail: String,
    pub style: ShapeStyle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeStyle {
    /// A community: filled, labelled when its label fits.
    Community,
    /// The outline of an expanded community; its members sit inside it.
    Hull,
    /// A member symbol of an expanded community; labelled on hover only.
    Member,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub source: usize,
    pub target: usize,
    /// Stroke width in layout units.
    pub width: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub width: f64,
    pub height: f64,
    pub shapes: Vec<Shape>,
    pub edges: Vec<Edge>,
}

/// Maps layout coordinates to CSS pixels of the canvas: uniform scale, centred.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    scale: f64,
    dx: f64,
    dy: f64,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            scale: 1.0,
            dx: 0.0,
            dy: 0.0,
        }
    }
}

impl Transform {
    fn fit(scene: &Scene, width: f64, height: f64) -> Self {
        if scene.width <= 0.0 || scene.height <= 0.0 {
            return Self::default();
        }
        let scale = (width / scene.width).min(height / scene.height);
        Self {
            scale,
            dx: (width - scene.width * scale) / 2.0,
            dy: (height - scene.height * scale) / 2.0,
        }
    }

    pub fn to_layout(self, point: Point) -> Point {
        Point::new(
            (point.x - self.dx) / self.scale,
            (point.y - self.dy) / self.scale,
        )
    }
}

/// Colours and font read from the page's CSS custom properties, so the canvas follows the
/// light or dark theme.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    text: String,
    muted: String,
    surface: String,
    border: String,
    edge: String,
    accent: String,
    font: String,
    palette: Vec<String>,
}

impl Theme {
    pub fn read() -> Self {
        let style = web_sys::window().and_then(|window| {
            let root = window.document()?.document_element()?;
            window.get_computed_style(&root).ok().flatten()
        });
        let var = |name: &str, fallback: &str| {
            style
                .as_ref()
                .and_then(|style| style.get_property_value(name).ok())
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| fallback.to_owned())
        };
        Self {
            text: var("--text", "#1c1f24"),
            muted: var("--muted", "#6b7280"),
            surface: var("--surface", "#ffffff"),
            border: var("--border", "#e5e7eb"),
            edge: var("--edge", "#c3c8d0"),
            accent: var("--accent", "#3b6fd8"),
            font: var("--font-sans", "system-ui, sans-serif"),
            palette: (0..8)
                .map(|index| var(&format!("--c{index}"), "#8a94a6"))
                .collect(),
        }
    }

    fn color(&self, index: usize) -> &str {
        self.palette
            .get(index % self.palette.len().max(1))
            .map_or("#8a94a6", String::as_str)
    }
}

/// Paints `scene` onto `canvas` at its current CSS size and returns the transform used, for
/// hit-testing pointer positions. `hovered` highlights one shape and the edges touching it.
pub fn draw(
    canvas: &HtmlCanvasElement,
    scene: &Scene,
    hovered: Option<usize>,
    theme: &Theme,
) -> Result<Transform, JsValue> {
    let width = f64::from(canvas.client_width());
    let height = f64::from(canvas.client_height());
    let ratio = web_sys::window().map_or(1.0, |window| window.device_pixel_ratio());
    let pixel_width = (width * ratio).round() as u32;
    let pixel_height = (height * ratio).round() as u32;
    if canvas.width() != pixel_width || canvas.height() != pixel_height {
        canvas.set_width(pixel_width);
        canvas.set_height(pixel_height);
    }
    let context: CanvasRenderingContext2d = canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("no 2d context"))?
        .dyn_into()?;
    context.set_transform(ratio, 0.0, 0.0, ratio, 0.0, 0.0)?;
    context.clear_rect(0.0, 0.0, width, height);
    let transform = Transform::fit(scene, width, height);
    let to_screen = |point: Point| {
        Point::new(
            point.x * transform.scale + transform.dx,
            point.y * transform.scale + transform.dy,
        )
    };
    let scale = transform.scale;

    for shape in scene.shapes.iter().filter(|s| s.style == ShapeStyle::Hull) {
        let at = to_screen(shape.at);
        context.begin_path();
        context.arc(at.x, at.y, shape.radius * scale, 0.0, TAU)?;
        context.set_fill_style_str(theme.color(shape.color));
        context.set_global_alpha(0.08);
        context.fill();
        context.set_global_alpha(1.0);
        context.set_stroke_style_str(theme.color(shape.color));
        context.set_line_width(1.25);
        context.stroke();
    }

    context.set_line_cap("round");
    for edge in &scene.edges {
        let (Some(source), Some(target)) =
            (scene.shapes.get(edge.source), scene.shapes.get(edge.target))
        else {
            continue;
        };
        let touches = hovered.is_some_and(|h| h == edge.source || h == edge.target);
        let (from, to) = (to_screen(source.at), to_screen(target.at));
        let (from, to) = (
            toward(from, to, source.radius * scale),
            toward(to, from, target.radius * scale),
        );
        context.begin_path();
        context.move_to(from.x, from.y);
        context.line_to(to.x, to.y);
        context.set_stroke_style_str(if touches { &theme.accent } else { &theme.edge });
        context.set_global_alpha(match hovered {
            Some(_) if !touches => 0.35,
            _ => 0.9,
        });
        context.set_line_width((edge.width * scale).max(0.75));
        context.stroke();
    }
    context.set_global_alpha(1.0);

    for (index, shape) in scene.shapes.iter().enumerate() {
        if shape.style == ShapeStyle::Hull {
            continue;
        }
        let at = to_screen(shape.at);
        let radius = (shape.radius * scale).max(2.0);
        let color = theme.color(shape.color);
        context.begin_path();
        context.arc(at.x, at.y, radius, 0.0, TAU)?;
        context.set_fill_style_str(color);
        context.set_global_alpha(if shape.style == ShapeStyle::Community {
            0.85
        } else {
            0.95
        });
        context.fill();
        context.set_global_alpha(1.0);
        context.set_stroke_style_str(if hovered == Some(index) {
            &theme.text
        } else {
            &theme.surface
        });
        context.set_line_width(if hovered == Some(index) { 2.0 } else { 1.5 });
        context.stroke();
    }

    draw_labels(&context, scene, &transform, hovered, theme)?;
    if let Some(shape) = hovered.and_then(|index| scene.shapes.get(index)) {
        draw_tooltip(
            &context,
            shape,
            to_screen(shape.at),
            shape.radius * scale,
            width,
            theme,
        )?;
    }
    Ok(transform)
}

/// The point `distance` along the segment from `from` to `to`.
fn toward(from: Point, to: Point, distance: f64) -> Point {
    let length = from.distance(to);
    if length <= distance || length <= f64::EPSILON {
        return from;
    }
    Point::new(
        from.x + (to.x - from.x) / length * distance,
        from.y + (to.y - from.y) / length * distance,
    )
}

#[derive(Clone, Copy)]
struct Rect {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Rect {
    fn overlaps(self, other: Rect) -> bool {
        self.left < other.right
            && other.left < self.right
            && self.top < other.bottom
            && other.top < self.bottom
    }
}

/// Labels under communities, larger ones first, skipping any that would overlap a placed
/// label or run off the canvas. Hull labels sit above the hull.
fn draw_labels(
    context: &CanvasRenderingContext2d,
    scene: &Scene,
    transform: &Transform,
    hovered: Option<usize>,
    theme: &Theme,
) -> Result<(), JsValue> {
    let scale = transform.scale;
    let width = scene.width * scale + 2.0 * transform.dx;
    let height = scene.height * scale + 2.0 * transform.dy;
    let mut order: Vec<usize> = (0..scene.shapes.len())
        .filter(|&index| scene.shapes[index].style != ShapeStyle::Member && Some(index) != hovered)
        .collect();
    order.sort_by(|&a, &b| {
        let rank = |index: usize| {
            let shape = &scene.shapes[index];
            (shape.style != ShapeStyle::Hull, -shape.radius)
        };
        rank(a)
            .partial_cmp(&rank(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut placed: Vec<Rect> = Vec::new();
    let line = 15.0;
    context.set_text_align("center");
    context.set_text_baseline("middle");
    for index in order {
        let shape = &scene.shapes[index];
        let hull = shape.style == ShapeStyle::Hull;
        context.set_font(&format!(
            "{} {}px {}",
            if hull { 600 } else { 500 },
            if hull { 13 } else { 12 },
            theme.font
        ));
        let text_width = context.measure_text(&shape.label)?.width();
        let center = Point::new(
            shape.at.x * scale + transform.dx,
            shape.at.y * scale + transform.dy,
        );
        let radius = shape.radius * scale;
        let y = if hull {
            center.y - radius - 12.0
        } else {
            center.y + radius + 10.0
        };
        let rect = Rect {
            left: center.x - text_width / 2.0 - 3.0,
            right: center.x + text_width / 2.0 + 3.0,
            top: y - line / 2.0,
            bottom: y + line / 2.0,
        };
        let outside =
            rect.left < 0.0 || rect.right > width || rect.top < 0.0 || rect.bottom > height;
        if outside || placed.iter().any(|other| other.overlaps(rect)) {
            continue;
        }
        placed.push(rect);
        context.set_fill_style_str(if hull { &theme.text } else { &theme.muted });
        context.fill_text(&shape.label, center.x, y)?;
    }
    Ok(())
}

fn draw_tooltip(
    context: &CanvasRenderingContext2d,
    shape: &Shape,
    at: Point,
    radius: f64,
    width: f64,
    theme: &Theme,
) -> Result<(), JsValue> {
    let title_font = format!("600 12px {}", theme.font);
    let detail_font = format!("400 11px {}", theme.font);
    context.set_font(&title_font);
    let title_width = context.measure_text(&shape.label)?.width();
    context.set_font(&detail_font);
    let detail_width = context.measure_text(&shape.detail)?.width();
    let box_width = title_width.max(detail_width) + 20.0;
    let box_height = if shape.detail.is_empty() { 26.0 } else { 42.0 };
    let left = (at.x - box_width / 2.0).clamp(4.0, (width - box_width - 4.0).max(4.0));
    let top = if at.y - radius - box_height - 8.0 > 4.0 {
        at.y - radius - box_height - 8.0
    } else {
        at.y + radius + 8.0
    };
    context.begin_path();
    context.round_rect_with_f64(left, top, box_width, box_height, 6.0)?;
    context.set_fill_style_str(&theme.surface);
    context.fill();
    context.set_stroke_style_str(&theme.border);
    context.set_line_width(1.0);
    context.stroke();
    context.set_text_align("left");
    context.set_text_baseline("middle");
    context.set_font(&title_font);
    context.set_fill_style_str(&theme.text);
    context.fill_text(&shape.label, left + 10.0, top + 13.0)?;
    if !shape.detail.is_empty() {
        context.set_font(&detail_font);
        context.set_fill_style_str(&theme.muted);
        context.fill_text(&shape.detail, left + 10.0, top + 29.0)?;
    }
    Ok(())
}

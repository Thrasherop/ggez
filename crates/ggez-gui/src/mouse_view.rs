//! Drawings of the mouse: one for choosing a button, one for previewing
//! the lighting. Both are painted from a few shapes, so they stay sharp at
//! any size and need no image files.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Ui, WidgetInfo, WidgetType,
};
use ggez_core::{Action, Button, ButtonMap};

use crate::theme::{
    self, ACCENT, ACCENT_DEEP, BG, LINE, RAISED, RAISED_HOVER, TEXT, TEXT_DIM, TEXT_FAINT,
};

const SHELL: Color32 = Color32::from_rgb(30, 35, 44);
const SHELL_EDGE: Color32 = Color32::from_rgb(58, 66, 80);

/// The mouse seen from above: a rounded shell, squarer at the shoulders
/// than an ellipse and a little fuller at the back.
#[derive(Clone, Copy)]
struct Body {
    center: Pos2,
    half_width: f32,
    front: f32,
    back: f32,
}

impl Body {
    fn top(&self) -> f32 {
        self.center.y - self.front
    }

    fn bottom(&self) -> f32 {
        self.center.y + self.back
    }

    /// Half the body's width at height `y`.
    fn half_width_at(&self, y: f32) -> f32 {
        // A superellipse: the higher the exponent, the squarer the corner.
        let (offset, radius, exponent) = if y < self.center.y {
            (self.center.y - y, self.front, 2.9)
        } else {
            (y - self.center.y, self.back, 2.3)
        };
        let t = (offset / radius).clamp(0.0, 1.0);
        self.half_width * (1.0 - t.powf(exponent)).powf(1.0 / exponent)
    }

    fn outline(&self) -> Vec<Pos2> {
        self.slice(self.top(), self.bottom(), -1.0, 1.0)
    }

    /// The part of the body between two heights and between two fractions
    /// of its width (-1 is the left edge, 1 the right edge).
    fn slice(&self, y0: f32, y1: f32, left: f32, right: f32) -> Vec<Pos2> {
        const STEPS: usize = 40;
        let ys: Vec<f32> = (0..=STEPS)
            .map(|i| {
                // Ease towards both ends, where the outline curves fastest.
                let t = i as f32 / STEPS as f32;
                let eased = 0.5 - 0.5 * (t * std::f32::consts::PI).cos();
                egui::lerp(y0..=y1, eased)
            })
            .collect();
        let edge =
            |fraction: f32, y: f32| pos2(self.center.x + fraction * self.half_width_at(y), y);
        let mut points: Vec<Pos2> = ys.iter().map(|&y| edge(right, y)).collect();
        points.extend(ys.iter().rev().map(|&y| edge(left, y)));
        points.dedup_by(|a, b| a.distance(*b) < 0.5);
        points
    }

    /// One side of the body between two heights: bounded by the outline on
    /// the outside and, on the inside, by a straight line `gap` away from
    /// the centre. `side` is -1 for the left half and 1 for the right.
    fn half(&self, y0: f32, y1: f32, side: f32, gap: f32) -> Vec<Pos2> {
        const STEPS: usize = 40;
        let mut points: Vec<Pos2> = (0..=STEPS)
            .map(|i| egui::lerp(y0..=y1, i as f32 / STEPS as f32))
            .filter(|&y| self.half_width_at(y) > gap)
            .map(|y| pos2(self.center.x + side * self.half_width_at(y), y))
            .collect();
        if let Some(first) = points.first().copied() {
            points.push(pos2(self.center.x + side * gap, y1));
            points.push(pos2(self.center.x + side * gap, first.y));
        }
        points
    }

    /// The perforations in the shell between two heights: staggered rows
    /// of holes.
    fn holes(&self, y0: f32, y1: f32, spacing: f32) -> Vec<Pos2> {
        let mut holes = Vec::new();
        let rows = ((y1 - y0) / (spacing * 0.9)).floor() as i32;
        for row in 0..=rows {
            let y = y0 + row as f32 * spacing * 0.9;
            let reach = self.half_width_at(y) - spacing * 1.1;
            let mut x = if row % 2 == 0 { 0.0 } else { spacing / 2.0 };
            while x <= reach {
                holes.push(pos2(self.center.x + x, y));
                if x > 0.0 {
                    holes.push(pos2(self.center.x - x, y));
                }
                x += spacing;
            }
        }
        holes
    }
}

fn fill(ui: &Ui, points: Vec<Pos2>, color: Color32) {
    // Slices of this shape are convex, which is what the painter requires.
    ui.painter()
        .add(Shape::convex_polygon(points, color, Stroke::NONE));
}

fn outline(ui: &Ui, points: Vec<Pos2>, stroke: Stroke) {
    ui.painter().add(Shape::closed_line(points, stroke));
}

/// The mark drawn on a small control. Painted rather than typed, because
/// not every font has arrow characters.
#[derive(Clone, Copy)]
enum Glyph {
    /// A triangle pointing along this direction.
    Arrow(f32, f32),
    /// The wheel: grip lines across a pill.
    Wheel,
    Text(&'static str),
}

impl Glyph {
    fn paint(self, ui: &Ui, rect: Rect, color: Color32) {
        let center = rect.center();
        match self {
            Glyph::Arrow(dx, dy) => {
                let (forward, side) = (vec2(dx, dy) * 4.0, vec2(-dy, dx) * 4.5);
                let points = vec![
                    center + forward,
                    center - forward + side,
                    center - forward - side,
                ];
                ui.painter()
                    .add(Shape::convex_polygon(points, color, Stroke::NONE));
            }
            Glyph::Wheel => {
                for i in -2..=2 {
                    let y = center.y + i as f32 * 7.0;
                    ui.painter().line_segment(
                        [pos2(rect.left() + 6.0, y), pos2(rect.right() - 6.0, y)],
                        Stroke::new(1.5, color),
                    );
                }
            }
            Glyph::Text(text) => {
                ui.painter().text(
                    center,
                    Align2::CENTER_CENTER,
                    text,
                    FontId::new(11.0, theme::strong()),
                    color,
                );
            }
        }
    }
}

/// How a binding reads in the small space of a diagram cell.
fn short(action: Action, describe: &dyn Fn(Action) -> String) -> String {
    describe(action).replace(" + ", "+")
}

/// The button picker. Draws every rebindable control and returns the one
/// that was clicked. `describe` turns a binding into words and `title`
/// names a button.
pub fn buttons(
    ui: &mut Ui,
    map: &ButtonMap,
    selected: Button,
    describe: &dyn Fn(Action) -> String,
    title: &dyn Fn(Button) -> String,
) -> Option<Button> {
    let factory = ButtonMap::default();
    let size = vec2(422.0, 388.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let origin = rect.min;
    let at = |x: f32, y: f32| origin + vec2(x, y);
    let mut clicked = None;
    let mut pointed_at = None;

    // One control: a hit area that reports whether it is selected and
    // whether the pointer is over it.
    let mut control = |ui: &mut Ui, button: Button, hit: Rect| -> (bool, bool) {
        let response = ui
            .interact(
                hit,
                ui.id().with(("mouse-control", button.to_string())),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!("{}: {}", title(button), describe(map.get(button))));
        response.widget_info(|| {
            WidgetInfo::selected(WidgetType::Button, true, button == selected, title(button))
        });
        theme::focus_ring(ui, &response, hit, 8);
        if response.clicked() {
            clicked = Some(button);
        }
        if response.hovered() {
            pointed_at = Some(button);
        }
        (button == selected, response.hovered())
    };
    let changed = |button: Button| map.get(button) != factory.get(button);
    let shade = |is_selected: bool, hovered: bool| {
        if is_selected {
            ACCENT_DEEP
        } else if hovered {
            RAISED_HOVER
        } else {
            RAISED
        }
    };
    let ink = |button: Button, is_selected: bool| {
        if is_selected || changed(button) {
            ACCENT
        } else {
            TEXT_DIM
        }
    };

    // --- The mouse, seen from above -------------------------------------
    let body = Body {
        center: at(322.0, 196.0),
        half_width: 98.0,
        front: 176.0,
        back: 176.0,
    };
    let cx = body.center.x;
    let split = body.top() + 172.0;
    fill(ui, body.outline(), SHELL);
    for hole in body.holes(split + 38.0, body.bottom() - 34.0, 13.0) {
        ui.painter().circle_filled(hole, 2.4, BG);
    }

    // Left and right buttons: the front of the shell, either side of the
    // centre line. Drawn first so the smaller controls on top win clicks.
    for (button, side, name) in [(Button::Left, -1.0, "Left"), (Button::Right, 1.0, "Right")] {
        let shape = body.half(body.top() + 2.0, split, side, 2.0);
        let (is_selected, hovered) = control(ui, button, Rect::from_points(&shape));
        if is_selected || hovered {
            fill(ui, shape.clone(), shade(is_selected, hovered));
        }
        if is_selected {
            outline(ui, shape, Stroke::new(1.5, ACCENT));
        }
        ui.painter().text(
            pos2(cx + side * 66.0, split - 16.0),
            Align2::CENTER_CENTER,
            name,
            FontId::new(13.0, theme::strong()),
            ink(button, is_selected),
        );
    }
    // The seams between the two buttons and the palm.
    let seam = Stroke::new(1.5, SHELL_EDGE);
    ui.painter()
        .line_segment([pos2(cx, body.top() + 2.0), pos2(cx, split)], seam);
    let reach = body.half_width_at(split);
    ui.painter()
        .line_segment([pos2(cx - reach, split), pos2(cx + reach, split)], seam);
    outline(ui, body.outline(), Stroke::new(1.5, SHELL_EDGE));

    // The wheel and what surrounds it, in a well between the buttons.
    let wheel_y = body.top() + 84.0;
    let cell = |x: f32, y: f32, w: f32, h: f32| Rect::from_center_size(pos2(x, y), vec2(w, h));
    ui.painter()
        .rect_filled(cell(cx, wheel_y, 90.0, 126.0), CornerRadius::same(16), BG);
    let mut small = |ui: &mut Ui, button: Button, rect: Rect, glyph: Glyph, radius: u8| {
        let (is_selected, hovered) = control(ui, button, rect.expand(2.0));
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(radius),
            shade(is_selected, hovered),
        );
        let stroke = if is_selected {
            Stroke::new(1.5, ACCENT)
        } else {
            Stroke::new(1.0, SHELL_EDGE)
        };
        ui.painter()
            .rect_stroke(rect, CornerRadius::same(radius), stroke, StrokeKind::Inside);
        glyph.paint(ui, rect, ink(button, is_selected));
    };
    small(
        ui,
        Button::ScrollUp,
        cell(cx, wheel_y - 46.0, 30.0, 22.0),
        Glyph::Arrow(0.0, -1.0),
        7,
    );
    small(
        ui,
        Button::Middle,
        cell(cx, wheel_y, 26.0, 58.0),
        Glyph::Wheel,
        12,
    );
    small(
        ui,
        Button::ScrollDown,
        cell(cx, wheel_y + 46.0, 30.0, 22.0),
        Glyph::Arrow(0.0, 1.0),
        7,
    );
    small(
        ui,
        Button::TiltLeft,
        cell(cx - 30.0, wheel_y, 24.0, 34.0),
        Glyph::Arrow(-1.0, 0.0),
        7,
    );
    small(
        ui,
        Button::TiltRight,
        cell(cx + 30.0, wheel_y, 24.0, 34.0),
        Glyph::Arrow(1.0, 0.0),
        7,
    );
    small(
        ui,
        Button::Dpi,
        cell(cx, split + 20.0, 46.0, 22.0),
        Glyph::Text("CPI"),
        7,
    );

    // --- The side grid, which sits under the thumb ------------------------
    let (cell_w, cell_h, gap) = (54.0, 44.0, 6.0);
    let grid_top = 112.0;
    ui.painter().text(
        at(0.0, grid_top - 20.0),
        Align2::LEFT_CENTER,
        "Side buttons",
        FontId::proportional(12.5),
        TEXT_FAINT,
    );
    for n in 1..=12u8 {
        let (row, col) = (f32::from((n - 1) / 3), f32::from((n - 1) % 3));
        let rect = Rect::from_min_size(
            at(col * (cell_w + gap), grid_top + row * (cell_h + gap)),
            vec2(cell_w, cell_h),
        );
        let button = Button::Grid(n);
        let (is_selected, hovered) = control(ui, button, rect);
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), shade(is_selected, hovered));
        if is_selected {
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(8),
                Stroke::new(1.5, ACCENT),
                StrokeKind::Inside,
            );
        }
        ui.painter().text(
            rect.left_top() + vec2(6.0, 4.0),
            Align2::LEFT_TOP,
            n.to_string(),
            FontId::proportional(10.5),
            TEXT_FAINT,
        );
        let text = short(map.get(button), describe);
        let font = FontId::new(
            if text.chars().count() > 6 { 10.5 } else { 13.0 },
            theme::strong(),
        );
        let color = if is_selected || changed(button) {
            ACCENT
        } else {
            TEXT
        };
        let galley = ui.painter().layout_no_wrap(text, font, color);
        let clip = ui.painter().with_clip_rect(rect.shrink(3.0));
        clip.galley(
            pos2(
                rect.center().x - galley.size().x / 2.0,
                rect.center().y + 5.0 - galley.size().y / 2.0,
            ),
            galley,
            color,
        );
    }
    // A hairline from the grid to the mouse's left flank, where it lives.
    let grid_right = at(
        3.0 * cell_w + 2.0 * gap + 8.0,
        grid_top + 2.0 * (cell_h + gap) - gap / 2.0,
    );
    let flank_y = split + 60.0;
    let flank = pos2(cx - body.half_width_at(flank_y), flank_y);
    ui.painter()
        .line_segment([grid_right, flank], Stroke::new(1.0, LINE));
    ui.painter().circle_filled(flank, 3.5, SHELL_EDGE);

    // Name whatever the pointer is over, or else the selected button, with
    // its binding: the small controls have no room for their own text.
    let shown = pointed_at.unwrap_or(selected);
    ui.painter().text(
        at(0.0, 340.0),
        Align2::LEFT_CENTER,
        title(shown),
        FontId::proportional(12.5),
        TEXT_FAINT,
    );
    ui.painter().text(
        at(0.0, 360.0),
        Align2::LEFT_CENTER,
        describe(map.get(shown)),
        FontId::new(14.5, theme::strong()),
        if changed(shown) { ACCENT } else { TEXT },
    );

    clicked
}

/// What the lights are doing, for the preview.
pub enum Lights {
    /// Front, palm, wrist.
    Zones([[u8; 3]; 3]),
    Rainbow,
}

/// Where the three zones sit in the preview, as fractions of its height:
/// front, palm, wrist.
pub const ZONE_BANDS: [(f32, f32); 3] = [(0.08, 0.35), (0.375, 0.645), (0.67, 0.92)];

/// The height of the lighting preview, so that controls beside it can be
/// lined up with the zones.
pub const LIGHTING_HEIGHT: f32 = 344.0;

/// The lighting preview: the mouse with its three zones lit through the
/// holes in its shell.
pub fn lighting(ui: &mut Ui, lights: &Lights) {
    let size = vec2(210.0, LIGHTING_HEIGHT);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let body = Body {
        center: rect.center(),
        half_width: 92.0,
        front: 166.0,
        back: 166.0,
    };
    fill(ui, body.outline(), Color32::from_rgb(16, 19, 24));

    let colors: [Color32; 3] = match lights {
        Lights::Zones(zones) => zones.map(|[r, g, b]| Color32::from_rgb(r, g, b)),
        Lights::Rainbow => {
            // Drift slowly through the hues, each zone a step behind.
            let time = ui.input(|input| input.time) as f32;
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
            [0.0, 0.12, 0.24].map(|offset| {
                egui::ecolor::Hsva::new((time * 0.08 + offset).fract(), 0.85, 1.0, 1.0).into()
            })
        }
    };
    let (top, height) = (body.top(), body.bottom() - body.top());
    for ((start, end), color) in ZONE_BANDS.into_iter().zip(colors) {
        let (y0, y1) = (top + start * height, top + end * height);
        // A soft glow behind the holes, then the holes themselves.
        fill(
            ui,
            body.slice(y0, y1, -0.94, 0.94),
            color.gamma_multiply(0.14),
        );
        for hole in body.holes(y0 + 9.0, y1 - 9.0, 12.0) {
            ui.painter()
                .circle_filled(hole, 4.2, color.gamma_multiply(0.25));
            ui.painter().circle_filled(hole, 2.6, color);
        }
    }
    outline(ui, body.outline(), Stroke::new(1.5, SHELL_EDGE));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> Body {
        Body {
            center: pos2(300.0, 200.0),
            half_width: 98.0,
            front: 176.0,
            back: 176.0,
        }
    }

    /// The painter fills these shapes as convex polygons; a dent would
    /// show up as a stray triangle of the wrong color.
    fn is_convex(points: &[Pos2]) -> bool {
        let n = points.len();
        let mut sign = 0.0f32;
        for i in 0..n {
            let (a, b, c) = (points[i], points[(i + 1) % n], points[(i + 2) % n]);
            let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);
            // Ignore nearly straight runs; rounding decides their sign.
            if cross.abs() > 0.05 {
                if sign != 0.0 && sign.signum() != cross.signum() {
                    return false;
                }
                sign = cross;
            }
        }
        true
    }

    #[test]
    fn every_filled_shape_is_convex() {
        let body = body();
        let split = body.top() + 172.0;
        assert!(is_convex(&body.outline()), "the shell");
        assert!(
            is_convex(&body.half(body.top() + 2.0, split, -1.0, 2.0)),
            "left button"
        );
        assert!(
            is_convex(&body.half(body.top() + 2.0, split, 1.0, 2.0)),
            "right button"
        );
        let height = body.bottom() - body.top();
        for (start, end) in ZONE_BANDS {
            let band = body.slice(
                body.top() + start * height,
                body.top() + end * height,
                -0.94,
                0.94,
            );
            assert!(is_convex(&band), "zone {start}-{end}");
        }
    }

    #[test]
    fn the_shell_is_widest_in_the_middle_and_closed_at_the_ends() {
        let body = body();
        assert!((body.half_width_at(body.center.y) - body.half_width).abs() < 0.01);
        assert!(body.half_width_at(body.top()) < 0.5);
        assert!(body.half_width_at(body.bottom()) < 0.5);
        // Squarer at the front than the back, as the real mouse is.
        let near = 150.0;
        assert!(
            body.half_width_at(body.center.y - near) > body.half_width_at(body.center.y + near)
        );
    }

    #[test]
    fn holes_stay_inside_the_shell() {
        let body = body();
        let holes = body.holes(body.center.y, body.bottom() - 34.0, 13.0);
        assert!(holes.len() > 50);
        for hole in holes {
            let margin = body.half_width_at(hole.y) - (hole.x - body.center.x).abs();
            assert!(
                margin >= 13.0,
                "a hole at {hole:?} is too close to the edge"
            );
        }
    }

    #[test]
    fn zones_are_in_order_and_do_not_overlap() {
        for pair in ZONE_BANDS.windows(2) {
            assert!(pair[0].1 < pair[1].0);
        }
        assert!(ZONE_BANDS[0].0 > 0.0 && ZONE_BANDS[2].1 < 1.0);
    }
}

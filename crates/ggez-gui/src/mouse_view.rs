//! Drawings of the mouse: one for choosing a button, one for previewing
//! the lighting. Both are painted from shapes rather than loaded from image
//! files, so they stay sharp at any size and can show the user's own colors.
//!
//! The mouse is lit from the upper left: its shell is brightest there and
//! falls away towards the lower right, its upper edge catches the light,
//! and it casts a soft shadow on whatever it rests on.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Mesh, Pos2, Rect, Sense, Shape,
    Stroke, StrokeKind, Ui, WidgetInfo, WidgetType,
};
use ggez_core::{Action, Button, ButtonMap};

use crate::theme::{
    self, ACCENT, EDGE, ON_SELECT, RAISED, RAISED_HOVER, SELECT, TEXT, TEXT_DIM, TEXT_FAINT,
};

const SHELL_LIGHT: Color32 = Color32::from_rgb(66, 70, 82);
const SHELL: Color32 = Color32::from_rgb(36, 39, 47);
const SHELL_DARK: Color32 = Color32::from_rgb(19, 21, 26);
const SHELL_RIM: Color32 = Color32::from_rgb(6, 7, 9);
const HOLE: Color32 = Color32::from_rgb(6, 7, 9);

/// The Aerox 9 seen from above, nose up. Its shell is about twice as long
/// as it is wide (128.8 mm by 68.2 mm with the thumb shelf), squarer at the
/// nose than at the back and widest a little behind the middle. The main
/// buttons are the front half; behind them the shell is an open lattice
/// that the lights shine through. On the left flank a shelf stands out,
/// carrying the twelve thumb buttons.
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

    fn height(&self) -> f32 {
        self.front + self.back
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

    /// The upper edge of the body, from `depth` below the nose on the left,
    /// over the nose, to the same height on the right, drawn `inset` inside
    /// the outline. This is where light catches the shell.
    fn crown(&self, depth: f32, inset: f32) -> Vec<Pos2> {
        const STEPS: usize = 30;
        let low = self.top() + depth;
        let point = |side: f32, t: f32| {
            let y = egui::lerp(low..=self.top() + inset, t * t.sqrt());
            let reach = (self.half_width_at(y - inset * 0.5) - inset).max(0.0);
            pos2(self.center.x + side * reach, y)
        };
        let mut points: Vec<Pos2> = (0..=STEPS)
            .map(|i| point(-1.0, i as f32 / STEPS as f32))
            .collect();
        points.extend(
            (0..=STEPS)
                .rev()
                .map(|i| point(1.0, i as f32 / STEPS as f32)),
        );
        points.dedup_by(|a, b| a.distance(*b) < 0.5);
        points
    }

    /// The openings of the lattice between two heights: staggered rows of
    /// diamonds `spacing` apart, each given by its middle.
    fn lattice(&self, y0: f32, y1: f32, spacing: f32) -> Vec<Pos2> {
        let pitch = spacing * LATTICE_PITCH;
        let tall = pitch - 1.0;
        let mut openings = Vec::new();
        let rows = ((y1 - y0) / pitch).floor() as i32;
        for row in 0..=rows {
            let y = y0 + row as f32 * pitch;
            // An opening has to clear the edge at its top and its bottom.
            let reach = self
                .half_width_at(y - tall)
                .min(self.half_width_at(y + tall))
                - spacing * 0.62;
            let mut x = if row % 2 == 0 { 0.0 } else { spacing / 2.0 };
            while x <= reach {
                openings.push(pos2(self.center.x + x, y));
                if x > 0.0 {
                    openings.push(pos2(self.center.x - x, y));
                }
                x += spacing;
            }
        }
        openings
    }

    /// How far the thumb shelf stands out from the shell's left edge at
    /// height `y`.
    fn shelf_reach(&self, y: f32) -> f32 {
        let along = ((y - self.top()) / self.height() - SHELF.0) / (SHELF.1 - SHELF.0);
        if !(0.0..=1.0).contains(&along) {
            return 0.0;
        }
        SHELF_DEPTH * self.height() * (along * std::f32::consts::PI).sin().powf(0.8)
    }

    /// The left edge of the mouse at height `y`, shelf included.
    fn left_edge(&self, y: f32) -> Pos2 {
        pos2(
            self.center.x - self.half_width_at(y) - self.shelf_reach(y),
            y,
        )
    }

    /// The thumb shelf: darker than the shell and lower, with grooves
    /// between its four groups of buttons.
    fn paint_shelf(&self, ui: &Ui) {
        const STEPS: usize = 36;
        let y_at = |step: f32| self.top() + (SHELF.0 + (SHELF.1 - SHELF.0) * step) * self.height();
        let mut mesh = Mesh::default();
        let mut outer = Vec::with_capacity(STEPS + 1);
        for step in 0..=STEPS {
            let y = y_at(step as f32 / STEPS as f32);
            let edge = self.left_edge(y);
            // The inner side is hidden under the shell, which is drawn next.
            let inner = pos2(self.center.x - self.half_width_at(y) + 10.0, y);
            mesh.colored_vertex(edge, SHELL_DARK);
            mesh.colored_vertex(inner, theme::lerp(SHELL_DARK, SHELL, 0.7));
            outer.push(edge);
        }
        for step in 0..STEPS as u32 {
            let i = step * 2;
            mesh.add_triangle(i, i + 1, i + 2);
            mesh.add_triangle(i + 1, i + 3, i + 2);
        }
        let painter = ui.painter();
        painter.add(Shape::mesh(mesh));
        for groove in [0.27, 0.5, 0.73] {
            let y = y_at(groove);
            painter.line_segment(
                [
                    self.left_edge(y - 0.012 * self.height()) + vec2(1.5, 0.0),
                    pos2(self.center.x - self.half_width_at(y), y),
                ],
                Stroke::new(1.3, SHELL_RIM),
            );
        }
        let lit: Vec<Pos2> = outer
            .iter()
            .take(STEPS * 2 / 3)
            .map(|&point| point + vec2(1.8, 0.6))
            .collect();
        painter.add(Shape::line(
            lit,
            Stroke::new(1.0, Color32::from_white_alpha(26)),
        ));
        painter.add(Shape::line(outer, Stroke::new(1.6, SHELL_RIM)));
    }

    /// The shadow the mouse casts, its thumb shelf, and its shell, lit from
    /// the upper left.
    fn paint_shell(&self, ui: &Ui) {
        let painter = ui.painter();
        painter.add(theme::halo(
            pos2(self.center.x + 2.0, self.top() + self.height() * 0.5 + 20.0),
            vec2(self.half_width * 1.5, self.height() * 0.6),
            Color32::from_black_alpha(190),
        ));
        self.paint_shelf(ui);
        let rim: Vec<(Pos2, Color32)> = self
            .outline()
            .into_iter()
            .map(|point| {
                let across = (point.x - self.center.x) / self.half_width;
                let down = (point.y - self.top()) / self.height();
                let shade = (0.5 * down + 0.25 * (across + 1.0)).clamp(0.0, 1.0);
                (point, theme::lerp(SHELL, SHELL_DARK, shade))
            })
            .collect();
        let hub = pos2(
            self.center.x - self.half_width * 0.24,
            self.center.y - self.front * 0.34,
        );
        painter.add(theme::fan((hub, SHELL_LIGHT), &rim));
    }

    /// The shell's edge: a dark line that also smooths the outline, and the
    /// light along its upper edge.
    fn paint_edge(&self, ui: &Ui) {
        let painter = ui.painter();
        painter.add(Shape::closed_line(
            self.outline(),
            Stroke::new(1.6, SHELL_RIM),
        ));
        painter.add(Shape::line(
            self.crown(self.front * 0.92, 2.2),
            Stroke::new(1.1, Color32::from_white_alpha(46)),
        ));
    }

    /// A groove in the shell: a dark line with light on its lower lip.
    fn paint_seam(ui: &Ui, from: Pos2, to: Pos2) {
        let along = (to - from).normalized();
        let lip = vec2(-along.y, along.x).abs() * 1.3;
        ui.painter().line_segment(
            [from + lip, to + lip],
            Stroke::new(1.0, Color32::from_white_alpha(22)),
        );
        ui.painter()
            .line_segment([from, to], Stroke::new(1.6, SHELL_RIM));
    }

    /// The well between the two main buttons that the wheel sits in.
    fn paint_well(ui: &Ui, well: Rect) {
        let painter = ui.painter();
        painter.rect_filled(well, CornerRadius::same(17), HOLE);
        painter.rect_stroke(
            well,
            CornerRadius::same(17),
            Stroke::new(1.0, Color32::from_white_alpha(16)),
            StrokeKind::Outside,
        );
    }
}

/// Where along the mouse's length the thumb shelf starts and ends, and how
/// far it stands out, all as fractions of that length.
const SHELF: (f32, f32) = (0.15, 0.8);
const SHELF_DEPTH: f32 = 0.053;

/// The distance between the lattice's rows, as a fraction of the distance
/// between openings in a row.
const LATTICE_PITCH: f32 = 0.62;

/// One opening of the lattice: a diamond around `middle`, dark, with the
/// light behind it showing in `glow` if there is any.
fn paint_opening(ui: &Ui, middle: Pos2, spacing: f32, glow: Option<Color32>) {
    let (wide, tall) = (spacing / 2.0 - 1.3, spacing * LATTICE_PITCH - 1.3);
    let diamond = |scale: f32, color: Color32| {
        Shape::convex_polygon(
            vec![
                middle - vec2(0.0, tall * scale),
                middle + vec2(wide * scale, 0.0),
                middle + vec2(0.0, tall * scale),
                middle - vec2(wide * scale, 0.0),
            ],
            color,
            Stroke::NONE,
        )
    };
    let painter = ui.painter();
    match glow {
        Some(color) => {
            painter.circle_filled(middle, tall * 1.25, color.gamma_multiply(0.16));
            painter.add(diamond(1.0, theme::lerp(HOLE, color, 0.82)));
            painter.add(diamond(0.55, theme::lerp(color, Color32::WHITE, 0.3)));
        }
        None => {
            painter.add(diamond(1.0, HOLE));
        }
    }
}

/// The wheel: a ribbed barrel, brightest along its middle.
fn paint_wheel(ui: &Ui, rect: Rect, fill: Color32, edge: Color32, ribs: Color32) {
    let painter = ui.painter();
    let radius = (rect.width() / 2.0).min(13.0);
    painter.add(theme::glow(
        rect.translate(vec2(0.0, 2.0)),
        radius,
        Color32::from_black_alpha(120),
        6.0,
    ));
    let middle = Rect::from_min_max(rect.min, pos2(rect.center().x, rect.max.y));
    painter.rect_filled(rect, CornerRadius::same(radius as u8), fill);
    painter.add(theme::gradient(
        middle.shrink2(vec2(0.0, 3.0)).translate(vec2(2.0, 0.0)),
        4.0,
        Color32::TRANSPARENT,
        Color32::from_white_alpha(24),
        theme::Along::Across,
    ));
    painter.rect_stroke(
        rect,
        CornerRadius::same(radius as u8),
        Stroke::new(1.0, edge),
        StrokeKind::Inside,
    );
    let count = ((rect.height() - 14.0) / 6.5).floor().max(1.0) as i32;
    for i in 0..count {
        let y = rect.center().y + (i as f32 - (count - 1) as f32 / 2.0) * 6.5;
        painter.line_segment(
            [pos2(rect.left() + 6.0, y), pos2(rect.right() - 6.0, y)],
            Stroke::new(1.6, ribs),
        );
    }
}

/// The mark drawn on a small control. Painted rather than typed, because
/// not every font has arrow characters.
#[derive(Clone, Copy)]
enum Glyph {
    /// A triangle pointing along this direction.
    Arrow(f32, f32),
    /// The wheel, which is drawn as one.
    Wheel,
    Text(&'static str),
}

impl Glyph {
    fn paint(self, ui: &Ui, rect: Rect, color: Color32) {
        let center = rect.center();
        match self {
            Glyph::Arrow(dx, dy) => {
                let (forward, side) = (vec2(dx, dy) * 3.2, vec2(-dy, dx) * 4.2);
                let points = vec![
                    center - forward + side,
                    center + forward,
                    center - forward - side,
                ];
                ui.painter()
                    .add(Shape::line(points, Stroke::new(1.7, color)));
            }
            Glyph::Wheel => {}
            Glyph::Text(text) => {
                ui.painter().text(
                    center,
                    Align2::CENTER_CENTER,
                    text,
                    FontId::new(10.5, theme::strong()),
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

/// What the lights are doing.
pub enum Lights {
    /// Front, palm, wrist.
    Zones([[u8; 3]; 3]),
    Rainbow,
}

impl Lights {
    /// The three zones' colors, dimmed to `level` (0 to 1). The rainbow
    /// drifts with `time`, in seconds.
    pub fn colors(&self, level: f32, time: f32) -> [Color32; 3] {
        let colors: [Color32; 3] = match self {
            Lights::Zones(zones) => zones.map(|[r, g, b]| Color32::from_rgb(r, g, b)),
            // Slowly through the hues, each zone a step behind.
            Lights::Rainbow => [0.0, 0.12, 0.24].map(|offset| {
                egui::ecolor::Hsva::new((time * 0.08 + offset).fract(), 0.85, 1.0, 1.0).into()
            }),
        };
        colors.map(|color| theme::lerp(Color32::BLACK, color, level.clamp(0.0, 1.0)))
    }
}

/// Where the drawing of the mouse was this frame: its middle and half its
/// height. The window lights the stage around that spot on the next frame.
pub fn take_focus(ctx: &egui::Context) -> Option<(Pos2, f32)> {
    ctx.data_mut(|data| data.remove_temp(egui::Id::new(FOCUS)))
}

const FOCUS: &str = "mouse-on-stage";

fn remember_focus(ui: &Ui, body: &Body) {
    let middle = pos2(body.center.x, body.top() + body.height() / 2.0);
    ui.ctx().data_mut(|data| {
        data.insert_temp(egui::Id::new(FOCUS), (middle, body.height() / 2.0));
    });
}

/// Where on the page a drawing of the mouse goes, and how large: `scale`
/// is 1 for its natural size.
#[derive(Clone, Copy)]
pub struct Place {
    pub center: Pos2,
    pub scale: f32,
}

/// A line from a label to the part of the mouse it names.
fn leader(ui: &Ui, from: Pos2, to: Pos2, lit: bool) {
    let color = if lit {
        SELECT
    } else {
        Color32::WHITE.gamma_multiply(0.32)
    };
    ui.painter()
        .line_segment([from, to], Stroke::new(1.0, color));
    ui.painter().circle_filled(to, 2.6, color);
}

/// The button picker: the mouse, placed and sized by `place`, with the
/// thumb buttons laid out beside it. Every
/// rebindable control can be clicked; returns the one that was.
/// `describe` turns a binding into words and `title` names a button; the
/// holes in the shell glow faintly in `lights`.
pub fn buttons(
    ui: &mut Ui,
    place: Place,
    map: &ButtonMap,
    selected: Button,
    lights: [Color32; 3],
    describe: &dyn Fn(Action) -> String,
    title: &dyn Fn(Button) -> String,
) -> Option<Button> {
    let Place { center, scale: k } = place;
    let factory = ButtonMap::default();
    let clicked = std::cell::Cell::new(None);

    // One control: a hit area that reports whether it is selected and
    // whether the pointer is over it.
    let control = |ui: &mut Ui, button: Button, hit: Rect| -> (bool, bool) {
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
            clicked.set(Some(button));
        }
        (button == selected, response.hovered())
    };
    let changed = |button: Button| map.get(button) != factory.get(button);
    // A key on the mouse: white when selected.
    let key = |ui: &Ui, rect: Rect, radius: u8, is_selected: bool, hovered: bool| {
        if is_selected {
            ui.painter().add(theme::glow(
                rect.expand(1.0),
                f32::from(radius),
                Color32::WHITE.gamma_multiply(0.3),
                16.0,
            ));
            ui.painter()
                .add(theme::raised(rect, radius, SELECT, Color32::WHITE));
        } else {
            let fill = if hovered { RAISED_HOVER } else { RAISED };
            ui.painter().add(theme::raised(rect, radius, fill, EDGE));
        }
    };
    let ink = |button: Button, is_selected: bool| {
        if is_selected {
            ON_SELECT
        } else if changed(button) {
            ACCENT
        } else {
            TEXT_DIM
        }
    };

    // --- The mouse, seen from above -------------------------------------
    let body = Body::placed(center, k * 356.0);
    remember_focus(ui, &body);
    let cx = body.center.x;
    let split = body.top() + SPLIT * body.height();
    body.paint_shell(ui);
    // The lattice: all of the palm, and the back of the main buttons either
    // side of the wheel. It glows faintly in the mouse's colors.
    let spacing = 14.0 * k;
    let mut openings = body.lattice(split + 36.0 * k, body.bottom() - 24.0 * k, spacing);
    openings.extend(
        body.lattice(body.top() + 122.0 * k, split - 12.0 * k, spacing)
            .into_iter()
            .filter(|opening| (opening.x - cx).abs() > 56.0 * k),
    );
    for opening in openings {
        let down = (opening.y - body.top()) / body.height();
        let zone = ZONE_BANDS
            .iter()
            .rposition(|(start, _)| down >= *start)
            .unwrap_or(0);
        paint_opening(ui, opening, spacing, None);
        ui.painter()
            .circle_filled(opening, 2.2 * k, lights[zone].gamma_multiply(0.5));
    }

    // Left and right buttons: the front of the shell, either side of the
    // centre line. Drawn first so the smaller controls on top win clicks.
    // Each has a label beside the mouse that can be clicked as well.
    let label_y = body.top() + 100.0 * k;
    for (button, side) in [(Button::Left, -1.0), (Button::Right, 1.0)] {
        let shape = body.half(body.top() + 3.0, split - 1.0, side, 2.0);
        let (is_selected, hovered) = control(ui, button, Rect::from_points(&shape));
        let anchor = pos2(cx + side * 63.0 * k, label_y);
        // Clear of the thumb shelf on the left.
        let edge = cx + side * (body.half_width + SHELF_DEPTH * body.height() + 26.0);
        let (pressed, over_label) = label(
            ui,
            button,
            &describe(map.get(button)),
            pos2(edge, label_y),
            side,
            anchor,
            is_selected,
            ink(button, false),
        );
        if pressed {
            clicked.set(Some(button));
        }
        // Slices of this shape are convex, which is what the painter needs.
        if is_selected {
            ui.painter().add(Shape::convex_polygon(
                shape.clone(),
                Color32::WHITE.gamma_multiply(0.13),
                Stroke::NONE,
            ));
            ui.painter()
                .add(Shape::closed_line(shape, Stroke::new(1.5, SELECT)));
        } else if hovered || over_label {
            ui.painter().add(Shape::convex_polygon(
                shape,
                Color32::WHITE.gamma_multiply(0.06),
                Stroke::NONE,
            ));
        }
    }
    // The seams between the two buttons and the palm.
    Body::paint_seam(ui, pos2(cx, body.top() + 3.0), pos2(cx, split));
    let reach = body.half_width_at(split) - 1.0;
    Body::paint_seam(ui, pos2(cx - reach, split), pos2(cx + reach, split));
    body.paint_edge(ui);

    // The wheel and what surrounds it, in a well between the buttons.
    let wheel_y = body.top() + 90.0 * k;
    let cell = |x: f32, y: f32, w: f32, h: f32| {
        Rect::from_center_size(pos2(cx + x * k, y), vec2(w * k, h * k))
    };
    Body::paint_well(ui, cell(0.0, wheel_y, 94.0, 132.0));
    let small = |ui: &mut Ui, button: Button, rect: Rect, glyph: Glyph, radius: u8| {
        let (is_selected, hovered) = control(ui, button, rect.expand(2.0));
        if matches!(glyph, Glyph::Wheel) {
            if is_selected {
                ui.painter().add(theme::glow(
                    rect.expand(1.0),
                    13.0,
                    Color32::WHITE.gamma_multiply(0.3),
                    16.0,
                ));
            }
            let (fill, edge) = match (is_selected, hovered) {
                (true, _) => (SELECT, Color32::WHITE),
                (false, true) => (RAISED_HOVER, EDGE),
                (false, false) => (RAISED, EDGE),
            };
            let ribs = if is_selected {
                ON_SELECT
            } else if changed(button) {
                ACCENT
            } else {
                TEXT_FAINT
            };
            paint_wheel(ui, rect, fill, edge, ribs);
        } else {
            key(ui, rect, radius, is_selected, hovered);
            glyph.paint(ui, rect, ink(button, is_selected));
        }
        is_selected
    };
    small(
        ui,
        Button::ScrollUp,
        cell(0.0, wheel_y - 48.0 * k, 32.0, 22.0),
        Glyph::Arrow(0.0, -1.0),
        7,
    );
    small(
        ui,
        Button::Middle,
        cell(0.0, wheel_y, 26.0, 60.0),
        Glyph::Wheel,
        12,
    );
    small(
        ui,
        Button::ScrollDown,
        cell(0.0, wheel_y + 48.0 * k, 32.0, 22.0),
        Glyph::Arrow(0.0, 1.0),
        7,
    );
    small(
        ui,
        Button::TiltLeft,
        cell(-31.0, wheel_y, 24.0, 36.0),
        Glyph::Arrow(-1.0, 0.0),
        7,
    );
    small(
        ui,
        Button::TiltRight,
        cell(31.0, wheel_y, 24.0, 36.0),
        Glyph::Arrow(1.0, 0.0),
        7,
    );
    let cpi = cell(0.0, split + 21.0 * k, 46.0, 22.0);
    let cpi_selected = small(ui, Button::Dpi, cpi, Glyph::Text("CPI"), 7);
    let (pressed, _) = label(
        ui,
        Button::Dpi,
        &describe(map.get(Button::Dpi)),
        pos2(
            cx + body.half_width + SHELF_DEPTH * body.height() + 26.0,
            cpi.center().y,
        ),
        1.0,
        cpi.right_center() + vec2(5.0, 0.0),
        cpi_selected,
        ink(Button::Dpi, false),
    );
    if pressed {
        clicked.set(Some(Button::Dpi));
    }

    // --- The side grid, which sits under the thumb ------------------------
    let (cell_w, cell_h, gap) = (58.0, 46.0, 6.0);
    let grid = vec2(3.0 * cell_w + 2.0 * gap, 4.0 * cell_h + 3.0 * gap);
    // The shelf is deepest about here.
    let flank_y = body.top() + (SHELF.0 + SHELF.1) / 2.0 * body.height();
    let grid_min = pos2(
        body.left_edge(flank_y).x - 86.0 - grid.x,
        (flank_y - grid.y / 2.0).max(label_y + 58.0),
    );
    let plate = Rect::from_min_size(grid_min, grid).expand(10.0);
    // A line from the grid to the thumb shelf, where its buttons are.
    let flank = body.left_edge(flank_y) + vec2(5.0, 0.0);
    let level = flank_y.clamp(plate.top() + 14.0, plate.bottom() - 14.0);
    ui.painter().add(Shape::line(
        vec![
            pos2(plate.right(), level),
            pos2(plate.right() + 22.0, level),
            flank,
        ],
        Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.32)),
    ));
    ui.painter()
        .circle_filled(flank, 2.6, Color32::WHITE.gamma_multiply(0.6));
    ui.painter().rect_filled(
        plate,
        CornerRadius::same(16),
        Color32::BLACK.gamma_multiply(0.35),
    );
    ui.painter().rect_stroke(
        plate,
        CornerRadius::same(16),
        Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.09)),
        StrokeKind::Inside,
    );
    ui.painter().text(
        plate.left_top() + vec2(4.0, -14.0),
        Align2::LEFT_CENTER,
        "Side buttons",
        FontId::new(11.5, theme::strong()),
        TEXT_FAINT,
    );
    for n in 1..=12u8 {
        let (row, col) = (f32::from((n - 1) / 3), f32::from((n - 1) % 3));
        let rect = Rect::from_min_size(
            grid_min + vec2(col * (cell_w + gap), row * (cell_h + gap)),
            vec2(cell_w, cell_h),
        );
        let button = Button::Grid(n);
        let (is_selected, hovered) = control(ui, button, rect);
        key(ui, rect, 10, is_selected, hovered);
        ui.painter().text(
            rect.left_top() + vec2(7.0, 5.0),
            Align2::LEFT_TOP,
            n.to_string(),
            FontId::proportional(10.5),
            if is_selected {
                theme::lerp(ON_SELECT, SELECT, 0.4)
            } else {
                TEXT_FAINT
            },
        );
        let text = short(map.get(button), describe);
        let font = FontId::new(
            if text.chars().count() > 6 { 10.5 } else { 13.0 },
            theme::strong(),
        );
        let color = if is_selected {
            ON_SELECT
        } else if changed(button) {
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

    clicked.get()
}

/// A label beside the mouse for one of its buttons, joined to it by a
/// line. It starts at `edge` and grows away from the mouse (`side` is -1
/// on the left, 1 on the right). Clicking it selects the button, like
/// clicking the button itself; returns whether it was clicked and whether
/// the pointer is over it.
#[allow(clippy::too_many_arguments)]
fn label(
    ui: &mut Ui,
    button: Button,
    text: &str,
    edge: Pos2,
    side: f32,
    anchor: Pos2,
    selected: bool,
    ink: Color32,
) -> (bool, bool) {
    let width = (ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::new(12.5, theme::strong()), ink)
        .size()
        .x
        + 26.0)
        .min(150.0);
    let rect = Rect::from_center_size(edge + vec2(side * width / 2.0, 0.0), vec2(width, 28.0));
    // The button itself is what assistive technology sees; this is a second
    // way for the pointer to reach it.
    let response = ui
        .interact(
            rect,
            ui.id().with(("mouse-label", button.to_string())),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    leader(ui, edge, anchor, selected);
    theme::tag(ui.painter(), rect, text, selected, response.hovered(), ink);
    (response.clicked(), response.hovered())
}

/// Where the three zones sit in the preview, as fractions of its height:
/// front, palm, wrist.
pub const ZONE_BANDS: [(f32, f32); 3] = [(0.25, 0.47), (0.5, 0.71), (0.735, 0.945)];

/// The color of the light at a point some fraction of the way down the
/// mouse: a zone's own color within the zone, a mix between two zones.
fn zone_color(lights: [Color32; 3], down: f32) -> Color32 {
    for (index, (start, end)) in ZONE_BANDS.into_iter().enumerate() {
        if down <= end {
            if down >= start || index == 0 {
                return lights[index];
            }
            let before = ZONE_BANDS[index - 1].1;
            let mix = (down - before) / (start - before);
            return theme::lerp(lights[index - 1], lights[index], mix);
        }
    }
    lights[2]
}

/// How far down the mouse its main buttons end and its palm begins.
const SPLIT: f32 = 0.485;

impl Body {
    /// The mouse `length` long with the middle of its length at `middle`.
    fn placed(middle: Pos2, length: f32) -> Self {
        // Widest 58% of the way back, and without the shelf not quite half
        // as wide as it is long.
        let front = 0.58 * length;
        Self {
            center: pos2(middle.x, middle.y - length / 2.0 + front),
            half_width: 0.236 * length,
            front,
            back: length - front,
        }
    }
}

/// The height of the lighting preview drawn at its natural size.
pub const LIGHTING_HEIGHT: f32 = 336.0;

/// The lighting preview: the mouse, placed and sized by `place`,
/// with its three zones shining through the holes in its shell in
/// `lights`. Returns a point on the mouse's right flank beside each zone,
/// for labels to attach to.
pub fn lighting(ui: &mut Ui, place: Place, lights: [Color32; 3]) -> [Pos2; 3] {
    let Place { center, scale: k } = place;
    let body = Body::placed(center, k * LIGHTING_HEIGHT);
    remember_focus(ui, &body);
    let lit = lights.iter().any(|&color| color != Color32::BLACK);
    let band = |(start, end): (f32, f32)| {
        (
            body.top() + start * body.height(),
            body.top() + end * body.height(),
        )
    };

    // Light spilling out from under the shell.
    if lit {
        for (zone, color) in ZONE_BANDS.into_iter().zip(lights) {
            let (y0, y1) = band(zone);
            let middle = (y0 + y1) / 2.0;
            ui.painter().add(theme::halo(
                pos2(body.center.x, middle),
                vec2(body.half_width_at(middle) * 1.5, (y1 - y0) * 0.9),
                color.gamma_multiply(0.5),
            ));
        }
    }
    body.paint_shell(ui);
    let split = body.top() + SPLIT * body.height();
    for (zone, color) in ZONE_BANDS.into_iter().zip(lights) {
        let (y0, y1) = band(zone);
        if lit {
            // The glow of the lights on the inside of the shell.
            let middle = (y0 + y1) / 2.0;
            ui.painter().add(theme::halo(
                pos2(body.center.x, middle),
                vec2(body.half_width_at(middle) * 0.92, (y1 - y0) * 0.62),
                color.gamma_multiply(0.2),
            ));
        }
    }
    // The lattice runs unbroken from the back of the main buttons to the
    // wrist. Between two zones their light mixes.
    let spacing = 13.0 * k;
    let (first, last) = (band(ZONE_BANDS[0]).0, band(ZONE_BANDS[2]).1);
    for opening in body.lattice(first + 8.0 * k, last - 8.0 * k, spacing) {
        // The strip between the main buttons is solid, and so is the seam
        // behind them.
        let in_strip = opening.y < split && (opening.x - body.center.x).abs() < 20.0 * k;
        let on_seam = (opening.y - split).abs() < spacing * LATTICE_PITCH + 2.0;
        if in_strip || on_seam {
            continue;
        }
        let color = zone_color(lights, (opening.y - body.top()) / body.height());
        paint_opening(ui, opening, spacing, lit.then_some(color));
    }
    // The seams around the main buttons, the wheel between them, and the
    // CPI button behind it.
    Body::paint_seam(
        ui,
        pos2(body.center.x, body.top() + 3.0),
        pos2(body.center.x, split),
    );
    let reach = body.half_width_at(split) - 1.0;
    Body::paint_seam(
        ui,
        pos2(body.center.x - reach, split),
        pos2(body.center.x + reach, split),
    );
    let wheel = Rect::from_center_size(
        pos2(body.center.x, body.top() + 0.19 * body.height()),
        vec2(22.0 * k, 52.0 * k),
    );
    Body::paint_well(ui, wheel.expand2(vec2(8.0, 9.0) * k));
    paint_wheel(ui, wheel, RAISED, EDGE, TEXT_FAINT);
    let cpi = Rect::from_center_size(
        pos2(body.center.x, body.top() + 0.385 * body.height()),
        vec2(15.0 * k, 26.0 * k),
    );
    ui.painter().add(theme::raised(cpi, 6, RAISED, EDGE));
    body.paint_edge(ui);

    ZONE_BANDS.map(|zone| {
        let (y0, y1) = band(zone);
        let middle = (y0 + y1) / 2.0;
        pos2(body.center.x + body.half_width_at(middle), middle)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> Body {
        Body::placed(pos2(300.0, 200.0), 356.0)
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
        let split = body.top() + SPLIT * body.height();
        assert!(is_convex(&body.outline()), "the shell");
        assert!(
            is_convex(&body.half(body.top() + 3.0, split - 1.0, -1.0, 2.0)),
            "left button"
        );
        assert!(
            is_convex(&body.half(body.top() + 3.0, split - 1.0, 1.0, 2.0)),
            "right button"
        );
    }

    #[test]
    fn the_mouse_has_the_proportions_of_the_real_one() {
        let body = body();
        assert!((body.height() - 356.0).abs() < 0.01);
        assert!((body.top() - 22.0).abs() < 0.01, "placed by its middle");
        // 128.8 mm long and 68.2 mm wide, thumb shelf included.
        let widest = (0..=356)
            .map(|step| {
                let y = body.top() + step as f32;
                body.center.x + body.half_width_at(y) - body.left_edge(y).x
            })
            .fold(0.0f32, f32::max);
        let ratio = body.height() / widest;
        assert!(
            (ratio - 128.8 / 68.2).abs() < 0.06,
            "length / width is {ratio}"
        );
    }

    #[test]
    fn the_thumb_shelf_is_on_the_left_flank_only() {
        let body = body();
        let at = |fraction: f32| body.shelf_reach(body.top() + fraction * body.height());
        assert_eq!(at(0.05), 0.0);
        assert_eq!(at(0.9), 0.0);
        assert!(at(0.2) > 0.0 && at(0.2) < at(0.45));
        assert!((at((SHELF.0 + SHELF.1) / 2.0) - SHELF_DEPTH * body.height()).abs() < 0.01);
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
    fn the_lattice_stays_inside_the_shell() {
        let body = body();
        let spacing = 14.0;
        let openings = body.lattice(body.center.y - 20.0, body.bottom() - 24.0, spacing);
        assert!(openings.len() > 80);
        let (wide, tall) = (spacing / 2.0 - 1.3, spacing * LATTICE_PITCH - 1.3);
        for opening in openings {
            // Each of the diamond's four points, with room for a strut.
            for (dx, dy) in [(wide, 0.0), (-wide, 0.0), (0.0, tall), (0.0, -tall)] {
                let (x, y) = (opening.x + dx, opening.y + dy);
                let margin = body.half_width_at(y) - (x - body.center.x).abs();
                assert!(margin >= 1.5, "an opening at {opening:?} reaches the edge");
            }
        }
    }

    #[test]
    fn the_light_on_the_upper_edge_stays_inside_the_shell() {
        let body = body();
        let crown = body.crown(body.front * 0.92, 2.2);
        assert!(crown.len() > 20);
        for point in crown {
            let margin = body.half_width_at(point.y) - (point.x - body.center.x).abs();
            assert!(margin >= 0.0, "the highlight at {point:?} is outside");
            assert!(point.y >= body.top());
        }
    }

    #[test]
    fn light_mixes_between_zones_and_is_pure_within_them() {
        let lights = [Color32::RED, Color32::GREEN, Color32::BLUE];
        assert_eq!(zone_color(lights, 0.0), Color32::RED);
        assert_eq!(zone_color(lights, 0.3), Color32::RED);
        assert_eq!(zone_color(lights, 0.6), Color32::GREEN);
        assert_eq!(zone_color(lights, 0.99), Color32::BLUE);
        let between = zone_color(lights, (ZONE_BANDS[0].1 + ZONE_BANDS[1].0) / 2.0);
        assert!(between.r() > 100 && between.g() > 100 && between.b() == 0);
    }

    #[test]
    fn zones_are_in_order_and_do_not_overlap() {
        for pair in ZONE_BANDS.windows(2) {
            assert!(pair[0].1 < pair[1].0);
        }
        assert!(ZONE_BANDS[0].0 > 0.0 && ZONE_BANDS[2].1 < 1.0);
    }
}

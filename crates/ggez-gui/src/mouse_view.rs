//! Drawings of the mouse: one for choosing a button, one for previewing
//! the lighting. Both are painted from shapes rather than loaded from image
//! files, so they stay sharp at any size and can show the user's own colors.
//!
//! The mouse is lit from the upper left: its shell is brightest there and
//! falls away towards the lower right, its upper edge catches the light,
//! and it casts a soft shadow on whatever it rests on.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Ui, WidgetInfo, WidgetType,
};
use ggez_core::{Action, Button, ButtonMap};

use crate::theme::{
    self, ACCENT, EDGE, LINE, RAISED, RAISED_HOVER, SUNKEN, TEXT, TEXT_DIM, TEXT_FAINT,
};

const SHELL_LIGHT: Color32 = Color32::from_rgb(66, 70, 82);
const SHELL: Color32 = Color32::from_rgb(36, 39, 47);
const SHELL_DARK: Color32 = Color32::from_rgb(19, 21, 26);
const SHELL_RIM: Color32 = Color32::from_rgb(6, 7, 9);
const HOLE: Color32 = Color32::from_rgb(6, 7, 9);

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

    /// The shadow the mouse casts and its shell, lit from the upper left.
    fn paint_shell(&self, ui: &Ui) {
        let painter = ui.painter();
        painter.add(theme::halo(
            pos2(self.center.x + 4.0, self.top() + self.height() * 0.5 + 20.0),
            vec2(self.half_width * 1.42, self.height() * 0.6),
            Color32::from_black_alpha(190),
        ));
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
    /// The three zones' colors now, dimmed to `level` (0 to 1). `moving`
    /// lets the rainbow drift, which means repainting all the time.
    fn colors(&self, ui: &Ui, level: f32, moving: bool) -> [Color32; 3] {
        let colors: [Color32; 3] = match self {
            Lights::Zones(zones) => zones.map(|[r, g, b]| Color32::from_rgb(r, g, b)),
            Lights::Rainbow => {
                // Drift slowly through the hues, each zone a step behind.
                let time = if moving {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(50));
                    ui.input(|input| input.time) as f32
                } else {
                    5.6
                };
                [0.0, 0.12, 0.24].map(|offset| {
                    egui::ecolor::Hsva::new((time * 0.08 + offset).fract(), 0.85, 1.0, 1.0).into()
                })
            }
        };
        colors.map(|color| theme::lerp(Color32::BLACK, color, level.clamp(0.0, 1.0)))
    }
}

/// The button picker. Draws every rebindable control and returns the one
/// that was clicked. `describe` turns a binding into words and `title`
/// names a button; the holes in the shell glow faintly in the user's
/// colors, at `level` (0 to 1).
pub fn buttons(
    ui: &mut Ui,
    map: &ButtonMap,
    selected: Button,
    lights: (&Lights, f32),
    describe: &dyn Fn(Action) -> String,
    title: &dyn Fn(Button) -> String,
) -> Option<Button> {
    let factory = ButtonMap::default();
    let size = vec2(436.0, 400.0);
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
    // A key on the mouse: lit in the accent color when selected.
    let key = |ui: &Ui, rect: Rect, radius: u8, is_selected: bool, hovered: bool| {
        if is_selected {
            ui.painter().add(theme::glow(
                rect.expand(1.0),
                f32::from(radius),
                ACCENT.gamma_multiply(0.45),
                14.0,
            ));
            ui.painter().add(theme::raised(
                rect,
                radius,
                theme::lerp(RAISED, ACCENT, 0.26),
                ACCENT,
            ));
        } else {
            let fill = if hovered { RAISED_HOVER } else { RAISED };
            ui.painter().add(theme::raised(rect, radius, fill, EDGE));
        }
    };
    let ink = |button: Button, is_selected: bool| {
        if is_selected {
            TEXT
        } else if changed(button) {
            ACCENT
        } else {
            TEXT_DIM
        }
    };

    // --- The mouse, seen from above -------------------------------------
    let body = Body {
        center: at(330.0, 208.0),
        half_width: 100.0,
        front: 186.0,
        back: 170.0,
    };
    let cx = body.center.x;
    let split = body.top() + 180.0;
    body.paint_shell(ui);
    let glow = lights.0.colors(ui, lights.1, false);
    for hole in body.holes(split + 40.0, body.bottom() - 34.0, 13.0) {
        let down = (hole.y - body.top()) / body.height();
        let zone = ZONE_BANDS
            .iter()
            .rposition(|(start, _)| down >= *start)
            .unwrap_or(0);
        ui.painter().circle_filled(hole, 2.7, HOLE);
        ui.painter()
            .circle_filled(hole, 1.7, glow[zone].gamma_multiply(0.5));
    }

    // Left and right buttons: the front of the shell, either side of the
    // centre line. Drawn first so the smaller controls on top win clicks.
    for (button, side, name) in [(Button::Left, -1.0, "Left"), (Button::Right, 1.0, "Right")] {
        let shape = body.half(body.top() + 3.0, split - 1.0, side, 2.0);
        let (is_selected, hovered) = control(ui, button, Rect::from_points(&shape));
        // Slices of this shape are convex, which is what the painter needs.
        if is_selected {
            ui.painter().add(Shape::convex_polygon(
                shape.clone(),
                ACCENT.gamma_multiply(0.2),
                Stroke::NONE,
            ));
            ui.painter()
                .add(Shape::closed_line(shape, Stroke::new(1.5, ACCENT)));
        } else if hovered {
            ui.painter().add(Shape::convex_polygon(
                shape,
                Color32::from_white_alpha(12),
                Stroke::NONE,
            ));
        }
        ui.painter().text(
            pos2(cx + side * 66.0, split - 18.0),
            Align2::CENTER_CENTER,
            name,
            FontId::new(12.5, theme::strong()),
            ink(button, is_selected),
        );
    }
    // The seams between the two buttons and the palm.
    Body::paint_seam(ui, pos2(cx, body.top() + 3.0), pos2(cx, split));
    let reach = body.half_width_at(split) - 1.0;
    Body::paint_seam(ui, pos2(cx - reach, split), pos2(cx + reach, split));
    body.paint_edge(ui);

    // The wheel and what surrounds it, in a well between the buttons.
    let wheel_y = body.top() + 90.0;
    let cell = |x: f32, y: f32, w: f32, h: f32| Rect::from_center_size(pos2(x, y), vec2(w, h));
    Body::paint_well(ui, cell(cx, wheel_y, 94.0, 132.0));
    let mut small = |ui: &mut Ui, button: Button, rect: Rect, glyph: Glyph, radius: u8| {
        let (is_selected, hovered) = control(ui, button, rect.expand(2.0));
        if matches!(glyph, Glyph::Wheel) {
            if is_selected {
                ui.painter().add(theme::glow(
                    rect.expand(1.0),
                    13.0,
                    ACCENT.gamma_multiply(0.45),
                    14.0,
                ));
            }
            let (fill, edge) = match (is_selected, hovered) {
                (true, _) => (theme::lerp(RAISED, ACCENT, 0.26), ACCENT),
                (false, true) => (RAISED_HOVER, EDGE),
                (false, false) => (RAISED, EDGE),
            };
            let ribs = if is_selected || changed(button) {
                ACCENT
            } else {
                TEXT_FAINT
            };
            paint_wheel(ui, rect, fill, edge, ribs);
        } else {
            key(ui, rect, radius, is_selected, hovered);
            glyph.paint(ui, rect, ink(button, is_selected));
        }
    };
    small(
        ui,
        Button::ScrollUp,
        cell(cx, wheel_y - 48.0, 32.0, 22.0),
        Glyph::Arrow(0.0, -1.0),
        7,
    );
    small(
        ui,
        Button::Middle,
        cell(cx, wheel_y, 26.0, 60.0),
        Glyph::Wheel,
        12,
    );
    small(
        ui,
        Button::ScrollDown,
        cell(cx, wheel_y + 48.0, 32.0, 22.0),
        Glyph::Arrow(0.0, 1.0),
        7,
    );
    small(
        ui,
        Button::TiltLeft,
        cell(cx - 31.0, wheel_y, 24.0, 36.0),
        Glyph::Arrow(-1.0, 0.0),
        7,
    );
    small(
        ui,
        Button::TiltRight,
        cell(cx + 31.0, wheel_y, 24.0, 36.0),
        Glyph::Arrow(1.0, 0.0),
        7,
    );
    small(
        ui,
        Button::Dpi,
        cell(cx, split + 21.0, 46.0, 22.0),
        Glyph::Text("CPI"),
        7,
    );

    // --- The side grid, which sits under the thumb ------------------------
    let (cell_w, cell_h, gap) = (56.0, 46.0, 6.0);
    let (grid_left, grid_top) = (10.0, 112.0);
    let plate = Rect::from_min_size(
        at(grid_left - 10.0, grid_top - 10.0),
        vec2(
            3.0 * cell_w + 2.0 * gap + 20.0,
            4.0 * cell_h + 3.0 * gap + 20.0,
        ),
    );
    // A line from the grid to the mouse's left flank, where it lives.
    let flank_y = split + 62.0;
    let flank = pos2(cx - body.half_width_at(flank_y) + 1.0, flank_y);
    let elbow = pos2(plate.right() + 18.0, flank_y);
    ui.painter().add(Shape::line(
        vec![pos2(plate.right(), flank_y), elbow, flank],
        Stroke::new(1.0, EDGE),
    ));
    ui.painter().circle_filled(flank, 3.5, ACCENT);
    ui.painter()
        .circle_stroke(flank, 6.5, Stroke::new(1.0, ACCENT.gamma_multiply(0.35)));
    ui.painter()
        .rect_filled(plate, CornerRadius::same(14), SUNKEN);
    ui.painter().rect_stroke(
        plate,
        CornerRadius::same(14),
        Stroke::new(1.0, LINE),
        StrokeKind::Inside,
    );
    ui.painter().text(
        plate.left_top() + vec2(2.0, -14.0),
        Align2::LEFT_CENTER,
        "Side buttons",
        FontId::new(11.5, theme::strong()),
        TEXT_FAINT,
    );
    for n in 1..=12u8 {
        let (row, col) = (f32::from((n - 1) / 3), f32::from((n - 1) % 3));
        let rect = Rect::from_min_size(
            at(
                grid_left + col * (cell_w + gap),
                grid_top + row * (cell_h + gap),
            ),
            vec2(cell_w, cell_h),
        );
        let button = Button::Grid(n);
        let (is_selected, hovered) = control(ui, button, rect);
        key(ui, rect, 9, is_selected, hovered);
        ui.painter().text(
            rect.left_top() + vec2(7.0, 5.0),
            Align2::LEFT_TOP,
            n.to_string(),
            FontId::proportional(10.5),
            if is_selected { TEXT_DIM } else { TEXT_FAINT },
        );
        let text = short(map.get(button), describe);
        let font = FontId::new(
            if text.chars().count() > 6 { 10.5 } else { 13.0 },
            theme::strong(),
        );
        let color = if is_selected {
            Color32::WHITE
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

    // Name whatever the pointer is over, or else the selected button, with
    // its binding: the small controls have no room for their own text.
    let shown = pointed_at.unwrap_or(selected);
    ui.painter().text(
        at(0.0, 352.0),
        Align2::LEFT_CENTER,
        title(shown),
        FontId::new(11.5, theme::strong()),
        TEXT_FAINT,
    );
    ui.painter().text(
        at(0.0, 372.0),
        Align2::LEFT_CENTER,
        describe(map.get(shown)),
        FontId::new(14.5, theme::strong()),
        if changed(shown) { ACCENT } else { TEXT },
    );

    clicked
}

/// Where the three zones sit in the preview, as fractions of its height:
/// front, palm, wrist.
pub const ZONE_BANDS: [(f32, f32); 3] = [(0.08, 0.35), (0.375, 0.645), (0.67, 0.92)];

/// The height of the lighting preview, so that controls beside it can be
/// lined up with the zones.
pub const LIGHTING_HEIGHT: f32 = 356.0;

/// The lighting preview: the mouse with its three zones shining through
/// the holes in its shell and spilling onto the desk, at `level` (0 to 1)
/// of full brightness.
pub fn lighting(ui: &mut Ui, lights: &Lights, level: f32) {
    let size = vec2(250.0, LIGHTING_HEIGHT);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let body = Body {
        center: rect.center() + vec2(0.0, 6.0),
        half_width: 92.0,
        front: 176.0,
        back: 160.0,
    };
    let colors = lights.colors(ui, level, true);
    let lit = level > 0.0;
    let band = |(start, end): (f32, f32)| {
        (
            body.top() + start * body.height(),
            body.top() + end * body.height(),
        )
    };

    // Light spilling out from under the shell.
    if lit {
        for (zone, color) in ZONE_BANDS.into_iter().zip(colors) {
            let (y0, y1) = band(zone);
            let middle = (y0 + y1) / 2.0;
            ui.painter().add(theme::halo(
                pos2(body.center.x, middle),
                vec2(body.half_width_at(middle) * 1.5, (y1 - y0) * 0.95),
                color.gamma_multiply(0.5),
            ));
        }
    }
    body.paint_shell(ui);
    for (zone, color) in ZONE_BANDS.into_iter().zip(colors) {
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
        for hole in body.holes(y0 + 9.0, y1 - 9.0, 12.0) {
            if lit {
                ui.painter()
                    .circle_filled(hole, 4.8, color.gamma_multiply(0.22));
                ui.painter().circle_filled(hole, 2.8, color);
                ui.painter().circle_filled(
                    hole,
                    1.2,
                    theme::lerp(color, Color32::WHITE, 0.45 * level),
                );
            } else {
                ui.painter().circle_filled(hole, 2.8, HOLE);
            }
        }
    }
    // Enough of the mouse's features to read as one: the seam between the
    // main buttons and the wheel.
    let split = body.top() + body.height() * (ZONE_BANDS[0].1 + ZONE_BANDS[1].0) / 2.0;
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
    let wheel = Rect::from_center_size(pos2(body.center.x, body.top() + 84.0), vec2(24.0, 56.0));
    Body::paint_well(ui, wheel.expand2(vec2(9.0, 10.0)));
    paint_wheel(ui, wheel, RAISED, EDGE, TEXT_FAINT);
    body.paint_edge(ui);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> Body {
        Body {
            center: pos2(300.0, 200.0),
            half_width: 100.0,
            front: 186.0,
            back: 170.0,
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
        let split = body.top() + 180.0;
        assert!(is_convex(&body.outline()), "the shell");
        assert!(
            is_convex(&body.half(body.top() + 3.0, split - 1.0, -1.0, 2.0)),
            "left button"
        );
        assert!(
            is_convex(&body.half(body.top() + 3.0, split - 1.0, 1.0, 2.0)),
            "right button"
        );
        for (start, end) in ZONE_BANDS {
            let band = body.slice(
                body.top() + start * body.height(),
                body.top() + end * body.height(),
                -0.95,
                0.95,
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
    fn zones_are_in_order_and_do_not_overlap() {
        for pair in ZONE_BANDS.windows(2) {
            assert!(pair[0].1 < pair[1].0);
        }
        assert!(ZONE_BANDS[0].0 > 0.0 && ZONE_BANDS[2].1 < 1.0);
    }
}

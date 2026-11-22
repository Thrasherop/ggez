//! The drawing of the mouse: once for choosing a button, once for
//! previewing the lighting. It is painted from shapes rather than loaded
//! from an image file, so it stays sharp at any size and can show the
//! user's own colors.
//!
//! The shapes follow the real Aerox 9 seen from above. Its outline and the
//! places of its parts were measured from SteelSeries' own top-down
//! photograph of the mouse and are kept here as fractions of its length:
//! two main buttons either side of a channel that holds the wheel, a solid
//! spine behind the wheel with the CPI button set into it, and around that
//! a shell cut into a lattice of square openings set on their corners,
//! which the lights shine through. The thumb buttons are on the left side
//! and show from above only as a few steps in the edge.
//!
//! The mouse is lit from the upper left and casts a soft shadow.

use eframe::egui::{
    self, pos2, vec2, Color32, CornerRadius, FontId, Mesh, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Ui, WidgetInfo, WidgetType,
};
use ggez_core::Button;

use crate::theme::{
    self, ACCENT, EDGE, ON_SELECT, RAISED, RAISED_HOVER, SELECT, TEXT, TEXT_DIM, TEXT_FAINT,
};

const SHELL_LIGHT: Color32 = Color32::from_rgb(70, 74, 86);
const SHELL: Color32 = Color32::from_rgb(38, 41, 49);
const SHELL_DARK: Color32 = Color32::from_rgb(19, 21, 26);
const SHELL_RIM: Color32 = Color32::from_rgb(6, 7, 9);
const HOLE: Color32 = Color32::from_rgb(6, 7, 9);
const RUBBER: Color32 = Color32::from_rgb(24, 26, 31);

/// The outline: how far the left and the right edge are from the centre
/// line at points along the mouse, nose first. Everything is a fraction of
/// the mouse's length.
const OUTLINE: [(f32, f32, f32); 35] = [
    (0.000, 0.052, 0.040),
    (0.008, 0.089, 0.072),
    (0.020, 0.117, 0.105),
    (0.031, 0.140, 0.128),
    (0.042, 0.159, 0.149),
    (0.053, 0.177, 0.166),
    (0.075, 0.208, 0.198),
    (0.098, 0.229, 0.212),
    (0.120, 0.243, 0.222),
    (0.142, 0.250, 0.226),
    (0.160, 0.252, 0.236),
    (0.176, 0.253, 0.246),
    (0.254, 0.251, 0.246),
    (0.355, 0.252, 0.243),
    (0.478, 0.254, 0.243),
    (0.522, 0.258, 0.246),
    (0.567, 0.262, 0.249),
    (0.601, 0.268, 0.253),
    (0.634, 0.271, 0.257),
    (0.690, 0.269, 0.264),
    (0.712, 0.270, 0.265),
    (0.735, 0.268, 0.263),
    (0.757, 0.261, 0.256),
    (0.779, 0.256, 0.251),
    (0.802, 0.250, 0.247),
    (0.824, 0.243, 0.240),
    (0.846, 0.233, 0.230),
    (0.869, 0.221, 0.218),
    (0.891, 0.205, 0.203),
    (0.913, 0.187, 0.186),
    (0.936, 0.168, 0.165),
    (0.958, 0.145, 0.142),
    (0.980, 0.116, 0.112),
    (0.992, 0.092, 0.088),
    (1.000, 0.052, 0.050),
];

/// The thumb buttons as they show from above: steps in the left edge, each
/// from one point along the mouse to another and standing out this far.
const THUMB_STEPS: [(f32, f32, f32); 3] = [
    (0.262, 0.336, 0.013),
    (0.346, 0.416, 0.009),
    (0.426, 0.492, 0.007),
];

/// Half the width of the channel between the main buttons.
const CHANNEL: f32 = 0.042;
/// Where the wheel is along the mouse, and half its width.
const WHEEL: (f32, f32) = (0.128, 0.262);
const WHEEL_HALF: f32 = 0.031;
/// The solid spine behind the wheel: where it ends, and half its width.
const SPINE_END: f32 = 0.462;
const SPINE_HALF: f32 = 0.04;
/// The CPI button, set into the spine.
const CPI: (f32, f32) = (0.327, 0.433);
const CPI_HALF: f32 = 0.025;
/// The seam behind the main buttons is this far along the mouse at its
/// edges and dips this much further back at the centre line.
const SEAM: f32 = 0.352;
const SEAM_DIP: f32 = 0.03;
/// The lattice: the distance between openings along a row, and the solid
/// margin left at the mouse's edge.
const PITCH: f32 = 0.052;
const RAIL: f32 = 0.027;
/// Where the lattice on the back of the main buttons starts, at its
/// nearest to the nose.
const BUTTON_LATTICE: f32 = 0.184;

/// Where the three light zones are along the mouse: front, palm, wrist.
pub const ZONE_BANDS: [(f32, f32); 3] = [(0.2, 0.35), (0.4, 0.66), (0.69, 0.965)];

/// The mouse at some place and size.
#[derive(Clone, Copy)]
struct Body {
    /// The tip of the nose, on the centre line.
    nose: Pos2,
    length: f32,
}

impl Body {
    /// The mouse `length` long with the middle of its length at `middle`.
    fn placed(middle: Pos2, length: f32) -> Self {
        Self {
            nose: pos2(middle.x, middle.y - length / 2.0),
            length,
        }
    }

    /// A length given as a fraction of the mouse's.
    fn of(&self, fraction: f32) -> f32 {
        fraction * self.length
    }

    /// The height on the page of a point some fraction along the mouse.
    fn y(&self, along: f32) -> f32 {
        self.nose.y + along * self.length
    }

    /// How far along the mouse a height on the page is.
    fn along(&self, y: f32) -> f32 {
        (y - self.nose.y) / self.length
    }

    /// A point given by how far across the mouse it is and how far along,
    /// both as fractions of the length; left of the centre line is negative.
    fn at(&self, across: f32, along: f32) -> Pos2 {
        pos2(self.nose.x + self.of(across), self.y(along))
    }

    /// How far the left and right edges are from the centre line at a
    /// point along the mouse, as fractions of the length.
    fn reach(along: f32) -> (f32, f32) {
        let along = along.clamp(0.0, 1.0);
        let after = OUTLINE
            .iter()
            .position(|(t, _, _)| *t >= along)
            .unwrap_or(OUTLINE.len() - 1)
            .max(1);
        let (t0, l0, r0) = OUTLINE[after - 1];
        let (t1, l1, r1) = OUTLINE[after];
        let mix = (along - t0) / (t1 - t0);
        (l0 + (l1 - l0) * mix, r0 + (r1 - r0) * mix)
    }

    /// How far one edge is from the centre line: the left edge for a
    /// negative `side`, the right edge otherwise.
    fn edge(side: f32, along: f32) -> f32 {
        let (left, right) = Self::reach(along);
        if side < 0.0 {
            left
        } else {
            right
        }
    }

    fn left(&self, along: f32) -> Pos2 {
        self.at(-Self::reach(along).0, along)
    }

    fn right(&self, along: f32) -> Pos2 {
        self.at(Self::reach(along).1, along)
    }

    /// The outline, clockwise from the nose.
    fn outline(&self) -> Vec<Pos2> {
        let mut points: Vec<Pos2> = OUTLINE.iter().map(|(t, _, _)| self.right(*t)).collect();
        points.extend(OUTLINE.iter().rev().map(|(t, _, _)| self.left(*t)));
        points
    }

    /// How far along the mouse the seam behind the main buttons is, at a
    /// distance from the centre line.
    fn seam(across: f32) -> f32 {
        let edge = Self::edge(across, SEAM);
        SEAM + SEAM_DIP * (1.0 - (across / edge).powi(2)).max(0.0)
    }

    /// One main button as rows across it, nose first: each row runs from
    /// the channel to the mouse's edge. `side` is -1 for the left button
    /// and 1 for the right.
    fn main_button(&self, side: f32) -> Vec<(Pos2, Pos2)> {
        const ROWS: usize = 44;
        (0..=ROWS)
            .filter_map(|row| {
                let along = 0.012 + (SEAM + SEAM_DIP - 0.012) * row as f32 / ROWS as f32;
                let mut edge = Self::edge(side, along) - 0.006;
                if along > SEAM {
                    // Behind the seam's ends only the middle is still button.
                    let behind = (1.0 - (along - SEAM) / SEAM_DIP).max(0.0).sqrt();
                    edge = edge.min(Self::edge(side, SEAM) * behind);
                }
                let inner = if along < WHEEL.1 { CHANNEL } else { SPINE_HALF } + 0.004;
                (edge > inner).then(|| (self.at(side * inner, along), self.at(side * edge, along)))
            })
            .collect()
    }

    /// Whether a point is where the shell is cut into lattice.
    fn is_lattice(across: f32, along: f32) -> bool {
        let edge = Self::edge(across, along);
        let off = across.abs();
        let seam = Self::seam(across);
        if along > seam + 0.008 {
            // The palm, all the way to the tail, around the spine.
            let on_spine = off < SPINE_HALF + 0.005 && along < SPINE_END + 0.008;
            along < 0.972 && off < edge - RAIL && !on_spine
        } else {
            // The back of a main button: a wedge that starts nearer the
            // nose beside the wheel than at the mouse's edge.
            let start = BUTTON_LATTICE + 0.3 * (off - 0.055);
            along > start && along < seam - 0.008 && off > 0.055 && off < edge - 0.034
        }
    }

    /// How far an opening of the lattice reaches from its middle to each of
    /// its corners, as a fraction of the mouse's length.
    fn opening_reach() -> f32 {
        PITCH / 2.0 - 0.0038
    }

    /// The middles of the lattice's openings: rows half a pitch apart, every
    /// other row shifted by half a pitch, wherever a whole opening fits.
    fn lattice(&self) -> Vec<Pos2> {
        let half = PITCH / 2.0;
        let reach = Self::opening_reach();
        let mut openings = Vec::new();
        let rows = ((0.98 - BUTTON_LATTICE) / half) as i32;
        for row in 0..=rows {
            let along = BUTTON_LATTICE + row as f32 * half;
            let shift = if row % 2 == 0 { 0.0 } else { half };
            for column in -6..=6 {
                let across = column as f32 * PITCH + shift;
                let fits = [
                    (0.0, 0.0),
                    (reach, 0.0),
                    (-reach, 0.0),
                    (0.0, reach),
                    (0.0, -reach),
                ]
                .into_iter()
                .all(|(dx, dy)| Self::is_lattice(across + dx, along + dy));
                if fits {
                    openings.push(self.at(across, along));
                }
            }
        }
        openings
    }

    /// The wheel's place on the page.
    fn wheel(&self) -> Rect {
        Rect::from_min_max(self.at(-WHEEL_HALF, WHEEL.0), self.at(WHEEL_HALF, WHEEL.1))
    }

    /// The CPI button's place on the page.
    fn cpi(&self) -> Rect {
        Rect::from_min_max(self.at(-CPI_HALF, CPI.0), self.at(CPI_HALF, CPI.1))
    }

    /// The left edge at a point along the mouse, thumb buttons included.
    fn thumb_edge(&self, along: f32) -> Pos2 {
        let step = THUMB_STEPS
            .iter()
            .find(|(from, to, _)| (*from..=*to).contains(&along))
            .map_or(0.0, |(_, _, out)| *out);
        self.left(along) - vec2(self.of(step), 0.0)
    }

    /// The whole mouse, apart from its wheel and CPI button and anything a
    /// page adds on top. With `glow`, the lattice shows each zone's light.
    fn paint(&self, ui: &Ui, glow: Option<[Color32; 3]>) {
        let painter = ui.painter();

        // Its shadow on the desk.
        painter.add(theme::halo(
            self.at(0.01, 0.55),
            vec2(self.of(0.4), self.of(0.6)),
            Color32::from_black_alpha(190),
        ));
        // The thumb buttons, which the shell overlaps.
        for (from, to, out) in THUMB_STEPS {
            let step = Rect::from_min_max(
                self.left(from) - vec2(self.of(out), 0.0),
                self.left(to) + vec2(self.of(0.03), 0.0),
            );
            painter.rect_filled(step, CornerRadius::same(3), SHELL_DARK);
            painter.rect_stroke(
                step,
                CornerRadius::same(3),
                Stroke::new(1.2, SHELL_RIM),
                StrokeKind::Inside,
            );
        }

        // The shell, brightest left of the centre line and towards the nose.
        const ROWS: usize = 80;
        let mut mesh = Mesh::default();
        for row in 0..=ROWS {
            let along = row as f32 / ROWS as f32;
            let (left, right) = Self::reach(along);
            let shade = |across: f32| {
                let side = (across / right.max(left) + 1.0) / 2.0;
                let dark = (0.55 * along + 0.4 * side).clamp(0.0, 1.0);
                theme::lerp(SHELL, SHELL_DARK, dark)
            };
            let lit = theme::lerp(SHELL_LIGHT, SHELL, (along * 1.15).min(1.0));
            let columns = [
                (-left, shade(-left)),
                (-left * 0.42, lit),
                (right * 0.4, shade(right * 0.2)),
                (right, shade(right)),
            ];
            for (across, color) in columns {
                mesh.colored_vertex(self.at(across, along), color);
            }
        }
        for row in 0..ROWS as u32 {
            for column in 0..3 {
                let i = row * 4 + column;
                mesh.add_triangle(i, i + 1, i + 4);
                mesh.add_triangle(i + 1, i + 5, i + 4);
            }
        }
        painter.add(Shape::mesh(mesh));

        // The light inside, seen as a glow on the shell around each zone.
        if let Some(colors) = glow {
            for ((from, to), color) in ZONE_BANDS.into_iter().zip(colors) {
                let along = (from + to) / 2.0;
                let (left, right) = Self::reach(along);
                painter.add(theme::halo(
                    self.at((right - left) / 2.0, along),
                    vec2(
                        self.of((left + right) / 2.0 * 0.9),
                        self.of((to - from) * 0.62),
                    ),
                    color.gamma_multiply(0.2),
                ));
            }
        }

        // The lattice.
        let reach = self.of(Self::opening_reach());
        for opening in self.lattice() {
            let color = glow.map(|colors| zone_color(colors, self.along(opening.y)));
            paint_opening(ui, opening, reach, color);
        }

        // The side walls, set a little in from the edge.
        for side in [-1.0f32, 1.0] {
            let wall: Vec<Pos2> = (0..=40)
                .map(|step| {
                    let along = 0.17 + 0.6 * step as f32 / 40.0;
                    self.at(side * (Self::edge(side, along) - 0.027), along)
                })
                .collect();
            painter.add(Shape::line(wall, Stroke::new(1.1, SHELL_RIM)));
        }

        // The channel between the main buttons, and the spine behind it.
        let channel =
            Rect::from_min_max(self.at(-CHANNEL, 0.004), self.at(CHANNEL, WHEEL.1 + 0.01));
        painter.rect_filled(channel, CornerRadius::same(2), HOLE);
        let spine = Rect::from_min_max(
            self.at(-SPINE_HALF, WHEEL.1 + 0.004),
            self.at(SPINE_HALF, SPINE_END),
        );
        let end = (self.of(SPINE_HALF) * 0.9) as u8;
        let round = CornerRadius {
            nw: 3,
            ne: 3,
            sw: end,
            se: end,
        };
        painter.rect_filled(spine, round, theme::lerp(SHELL, SHELL_LIGHT, 0.22));
        painter.rect_stroke(
            spine,
            round,
            Stroke::new(1.2, SHELL_RIM),
            StrokeKind::Inside,
        );

        // The seam behind the main buttons, either side of the spine.
        for side in [-1.0f32, 1.0] {
            let edge = Self::edge(side, SEAM);
            let seam: Vec<Pos2> = (0..=24)
                .map(|step| {
                    let across = side * (SPINE_HALF + (edge - SPINE_HALF) * step as f32 / 24.0);
                    self.at(across, Self::seam(across))
                })
                .collect();
            let lip: Vec<Pos2> = seam.iter().map(|&point| point + vec2(0.0, 1.3)).collect();
            painter.add(Shape::line(
                lip,
                Stroke::new(1.0, Color32::from_white_alpha(20)),
            ));
            painter.add(Shape::line(seam, Stroke::new(1.6, SHELL_RIM)));
        }

        // The mouse's edge: a dark line, with light on its upper left.
        painter.add(Shape::closed_line(
            self.outline(),
            Stroke::new(1.7, SHELL_RIM),
        ));
        let lit: Vec<Pos2> = OUTLINE
            .iter()
            .filter(|(along, _, _)| (0.02..=0.56).contains(along))
            .map(|(along, _, _)| self.left(*along) + vec2(2.1, 0.8))
            .collect();
        painter.add(Shape::line(
            lit,
            Stroke::new(1.0, Color32::from_white_alpha(44)),
        ));
    }

    /// The wheel: a rubber barrel with chevrons cut into it.
    fn paint_wheel(&self, ui: &Ui) {
        let painter = ui.painter();
        let wheel = self.wheel();
        let radius = CornerRadius::same((wheel.width() * 0.42) as u8);
        painter.rect_filled(wheel, radius, RUBBER);
        painter.add(theme::gradient(
            Rect::from_min_max(wheel.min, pos2(wheel.center().x, wheel.max.y))
                .shrink2(vec2(1.5, 4.0)),
            4.0,
            Color32::TRANSPARENT,
            Color32::from_white_alpha(18),
            theme::Along::Across,
        ));
        painter.rect_stroke(wheel, radius, Stroke::new(1.0, EDGE), StrokeKind::Inside);
        let (inset, rise) = (wheel.width() * 0.22, wheel.width() * 0.16);
        let count = ((wheel.height() - 12.0) / 7.0).floor().max(1.0) as i32;
        for i in 0..count {
            let y = wheel.center().y + (i as f32 - (count - 1) as f32 / 2.0) * 7.0;
            painter.add(Shape::line(
                vec![
                    pos2(wheel.left() + inset, y + rise),
                    pos2(wheel.center().x, y - rise),
                    pos2(wheel.right() - inset, y + rise),
                ],
                Stroke::new(1.4, TEXT_FAINT),
            ));
        }
    }

    /// The CPI button, a long pill.
    fn paint_cpi(&self, ui: &Ui, fill: Color32, edge: Color32) {
        let cpi = self.cpi();
        let radius = CornerRadius::same((cpi.width() / 2.0) as u8);
        ui.painter().rect_filled(cpi, radius, fill);
        ui.painter()
            .rect_stroke(cpi, radius, Stroke::new(1.0, edge), StrokeKind::Inside);
    }
}

/// A shape given as rows across it, filled with one color.
fn fill_rows(rows: &[(Pos2, Pos2)], color: Color32) -> Shape {
    let mut mesh = Mesh::default();
    for (from, to) in rows {
        mesh.colored_vertex(*from, color);
        mesh.colored_vertex(*to, color);
    }
    for row in 0..rows.len().saturating_sub(1) as u32 {
        let i = row * 2;
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i + 1, i + 3, i + 2);
    }
    Shape::mesh(mesh)
}

/// The edge of a shape given as rows across it.
fn rows_outline(rows: &[(Pos2, Pos2)]) -> Vec<Pos2> {
    let mut points: Vec<Pos2> = rows.iter().map(|(_, to)| *to).collect();
    points.extend(rows.iter().rev().map(|(from, _)| *from));
    points
}

/// One opening of the lattice: a square set on its corner, the corners
/// rounded off, reaching `reach` from its middle. It is dark, or shows the
/// light behind it.
fn paint_opening(ui: &Ui, middle: Pos2, reach: f32, glow: Option<Color32>) {
    let shape = |reach: f32, color: Color32| {
        let cut = reach * 0.15;
        let points = [
            (-cut, -reach + cut),
            (cut, -reach + cut),
            (reach - cut, -cut),
            (reach - cut, cut),
            (cut, reach - cut),
            (-cut, reach - cut),
            (-reach + cut, cut),
            (-reach + cut, -cut),
        ];
        Shape::convex_polygon(
            points.map(|(x, y)| middle + vec2(x, y)).to_vec(),
            color,
            Stroke::NONE,
        )
    };
    let painter = ui.painter();
    match glow {
        Some(color) => {
            // Brightest in the middle, where the light is nearest.
            painter.add(shape(reach, theme::lerp(HOLE, color, 0.7)));
            painter.add(shape(reach * 0.72, color));
        }
        None => {
            painter.add(shape(reach, HOLE));
        }
    }
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

/// The color of the light at a point some fraction of the way along the
/// mouse: a zone's own color within the zone, a mix between two zones.
fn zone_color(lights: [Color32; 3], along: f32) -> Color32 {
    for (index, (start, end)) in ZONE_BANDS.into_iter().enumerate() {
        if along <= end {
            if along >= start || index == 0 {
                return lights[index];
            }
            let before = ZONE_BANDS[index - 1].1;
            let mix = (along - before) / (start - before);
            return theme::lerp(lights[index - 1], lights[index], mix);
        }
    }
    lights[2]
}

/// Where the drawing of the mouse was this frame: its middle and half its
/// length. The window lights the stage around that spot on the next frame.
pub fn take_focus(ctx: &egui::Context) -> Option<(Pos2, f32)> {
    ctx.data_mut(|data| data.remove_temp(egui::Id::new(FOCUS)))
}

const FOCUS: &str = "mouse-on-stage";

fn remember_focus(ui: &Ui, body: &Body) {
    ui.ctx().data_mut(|data| {
        data.insert_temp(egui::Id::new(FOCUS), (body.at(0.0, 0.5), body.length / 2.0));
    });
}

/// Where on the page a drawing of the mouse goes, and how large: `scale`
/// is 1 for its natural size.
#[derive(Clone, Copy)]
pub struct Place {
    pub center: Pos2,
    pub scale: f32,
}

/// The length of the mouse on the buttons page at its natural size.
pub const BUTTONS_HEIGHT: f32 = 356.0;
/// The length of the mouse on the lighting page at its natural size.
pub const LIGHTING_HEIGHT: f32 = 336.0;

/// A label beside the mouse for one of its controls: where the label is,
/// and the spot on the mouse it points to.
struct Callout {
    button: Button,
    /// -1 for a label left of the mouse, 1 for one to its right.
    side: f32,
    /// Which line the label is on, counting down from the first.
    line: f32,
    /// The spot it points to: how far across the mouse and how far along.
    spot: (f32, f32),
}

const CALLOUTS: [Callout; 8] = [
    Callout {
        button: Button::Left,
        side: -1.0,
        line: 0.0,
        spot: (-0.15, 0.1),
    },
    Callout {
        button: Button::TiltLeft,
        side: -1.0,
        line: 1.0,
        spot: (-WHEEL_HALF - 0.008, 0.185),
    },
    Callout {
        button: Button::Right,
        side: 1.0,
        line: 0.0,
        spot: (0.15, 0.1),
    },
    Callout {
        button: Button::ScrollUp,
        side: 1.0,
        line: 1.0,
        spot: (0.012, WHEEL.0 + 0.02),
    },
    Callout {
        button: Button::TiltRight,
        side: 1.0,
        line: 2.0,
        spot: (WHEEL_HALF + 0.008, 0.185),
    },
    Callout {
        button: Button::Middle,
        side: 1.0,
        line: 3.0,
        spot: (0.012, 0.204),
    },
    Callout {
        button: Button::ScrollDown,
        side: 1.0,
        line: 4.0,
        spot: (0.012, WHEEL.1 - 0.02),
    },
    Callout {
        button: Button::Dpi,
        side: 1.0,
        line: 5.0,
        spot: (CPI_HALF - 0.006, 0.39),
    },
];

/// The button picker: the mouse, placed and sized by `place`, with a label
/// for each of its controls beside it and the thumb buttons laid out as a
/// grid. Every rebindable control can be clicked, on the mouse or by its
/// label; returns the one that was. `binding` says what a button does, in
/// words, and whether that differs from the factory layout; `title` names
/// a button; the lattice glows dimly in `lights`.
pub fn buttons(
    ui: &mut Ui,
    place: Place,
    selected: Button,
    lights: [Color32; 3],
    binding: &dyn Fn(Button) -> (String, bool),
    title: &dyn Fn(Button) -> String,
) -> Option<Button> {
    let clicked = std::cell::Cell::new(None);
    let body = Body::placed(place.center, place.scale * BUTTONS_HEIGHT);
    remember_focus(ui, &body);

    // One control on the mouse: a hit area that reports whether it is
    // selected and whether the pointer is over it.
    let control = |ui: &mut Ui, button: Button, hit: Rect| -> (bool, bool) {
        let response = ui
            .interact(
                hit,
                ui.id().with(("mouse-control", button.to_string())),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!("{}: {}", title(button), binding(button).0));
        response.widget_info(|| {
            WidgetInfo::selected(WidgetType::Button, true, button == selected, title(button))
        });
        theme::focus_ring(ui, &response, hit, 6);
        if response.clicked() {
            clicked.set(Some(button));
        }
        (button == selected, response.hovered())
    };

    let lit = lights != [Color32::BLACK; 3];
    let dim = lights.map(|color| theme::lerp(Color32::BLACK, color, 0.5));
    body.paint(ui, lit.then_some(dim));

    // The main buttons: everything in front of the seam, either side of the
    // channel. They come first so that the wheel, on top, wins clicks.
    for (button, side) in [(Button::Left, -1.0), (Button::Right, 1.0)] {
        let rows = body.main_button(side);
        let hit = Rect::from_points(&rows_outline(&rows));
        let (is_selected, hovered) = control(ui, button, hit);
        if is_selected {
            ui.painter()
                .add(fill_rows(&rows, Color32::WHITE.gamma_multiply(0.14)));
            ui.painter().add(Shape::closed_line(
                rows_outline(&rows),
                Stroke::new(1.5, SELECT),
            ));
        } else if hovered {
            ui.painter()
                .add(fill_rows(&rows, Color32::WHITE.gamma_multiply(0.06)));
        }
    }

    // The wheel: its front third scrolls up, its middle clicks, its back
    // third scrolls down, and a strip either side of it tilts.
    body.paint_wheel(ui);
    let wheel = body.wheel();
    let third = wheel.height() / 3.0;
    let slice = |index: f32| {
        Rect::from_min_size(
            wheel.left_top() + vec2(0.0, index * third),
            vec2(wheel.width(), third),
        )
    };
    let strip = body.of(0.03);
    let parts = [
        (Button::ScrollUp, slice(0.0)),
        (Button::Middle, slice(1.0)),
        (Button::ScrollDown, slice(2.0)),
        (
            Button::TiltLeft,
            Rect::from_min_max(wheel.left_top() - vec2(strip, 0.0), wheel.left_bottom()),
        ),
        (
            Button::TiltRight,
            Rect::from_min_max(wheel.right_top(), wheel.right_bottom() + vec2(strip, 0.0)),
        ),
    ];
    for (button, part) in parts {
        let (is_selected, hovered) = control(ui, button, part);
        let mark = part.shrink(1.5);
        if is_selected {
            ui.painter()
                .rect_filled(mark, CornerRadius::same(4), SELECT.gamma_multiply(0.85));
        } else if hovered {
            ui.painter().rect_filled(
                mark,
                CornerRadius::same(4),
                Color32::WHITE.gamma_multiply(0.22),
            );
        }
    }
    let (cpi_selected, cpi_hovered) = control(ui, Button::Dpi, body.cpi().expand(2.0));
    let (fill, edge) = match (cpi_selected, cpi_hovered) {
        (true, _) => (SELECT, Color32::WHITE),
        (false, true) => (RAISED_HOVER, EDGE),
        (false, false) => (RUBBER, EDGE),
    };
    body.paint_cpi(ui, fill, edge);

    // The labels beside the mouse.
    let first = body.y(0.06);
    let spacing = body.of(0.078).max(32.0);
    for callout in &CALLOUTS {
        let button = callout.button;
        let edge = pos2(
            body.nose.x + callout.side * (body.of(0.285) + 30.0),
            first + callout.line * spacing,
        );
        let (does, changed) = binding(button);
        let ink = if changed { ACCENT } else { TEXT_DIM };
        let pressed = label(
            ui,
            button,
            &does,
            edge,
            callout.side,
            body.at(callout.spot.0, callout.spot.1),
            button == selected,
            ink,
        );
        if pressed {
            clicked.set(Some(button));
        }
    }

    // --- The thumb buttons, as a grid beside the mouse --------------------
    let (cell_w, cell_h, gap) = (58.0, 46.0, 6.0);
    let grid = vec2(3.0 * cell_w + 2.0 * gap, 4.0 * cell_h + 3.0 * gap);
    // The middle of the steps in the mouse's left edge.
    let thumb = (THUMB_STEPS[0].0 + THUMB_STEPS[2].1) / 2.0;
    let flank = body.thumb_edge(thumb) + vec2(4.0, 0.0);
    let grid_min = pos2(
        body.nose.x - body.of(0.285) - 86.0 - grid.x,
        (flank.y - grid.y / 2.0).max(first + spacing + 52.0),
    );
    let plate = Rect::from_min_size(grid_min, grid).expand(10.0);
    let level = flank.y.clamp(plate.top() + 14.0, plate.bottom() - 14.0);
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
        egui::Align2::LEFT_CENTER,
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
        if is_selected {
            ui.painter().add(theme::glow(
                rect.expand(1.0),
                10.0,
                Color32::WHITE.gamma_multiply(0.3),
                16.0,
            ));
            ui.painter()
                .add(theme::raised(rect, 10, SELECT, Color32::WHITE));
        } else {
            let fill = if hovered { RAISED_HOVER } else { RAISED };
            ui.painter().add(theme::raised(rect, 10, fill, EDGE));
        }
        ui.painter().text(
            rect.left_top() + vec2(7.0, 5.0),
            egui::Align2::LEFT_TOP,
            n.to_string(),
            FontId::proportional(10.5),
            if is_selected {
                theme::lerp(ON_SELECT, SELECT, 0.4)
            } else {
                TEXT_FAINT
            },
        );
        // Shortened to fit a key: "Ctrl+X", not "Ctrl + X".
        let (does, changed) = binding(button);
        let text = does.replace(" + ", "+");
        let font = FontId::new(
            if text.chars().count() > 6 { 10.5 } else { 13.0 },
            theme::strong(),
        );
        let color = if is_selected {
            ON_SELECT
        } else if changed {
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

/// A label beside the mouse for one of its controls, joined by a line to
/// `spot` on the mouse. It starts at `edge` and grows away from the mouse
/// (`side` is -1 on the left, 1 on the right). Clicking it selects the
/// control, like clicking the control itself; returns whether it was
/// clicked.
#[allow(clippy::too_many_arguments)]
fn label(
    ui: &mut Ui,
    button: Button,
    text: &str,
    edge: Pos2,
    side: f32,
    spot: Pos2,
    selected: bool,
    ink: Color32,
) -> bool {
    let width = (ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::new(12.5, theme::strong()), ink)
        .size()
        .x
        + 26.0)
        .min(132.0);
    let rect = Rect::from_center_size(edge + vec2(side * width / 2.0, 0.0), vec2(width, 26.0));
    // The control on the mouse is what assistive technology sees; this is a
    // second, larger way for the pointer to reach it.
    let response = ui
        .interact(
            rect,
            ui.id().with(("mouse-label", button.to_string())),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let line = if selected {
        SELECT
    } else if response.hovered() {
        Color32::WHITE.gamma_multiply(0.6)
    } else {
        Color32::WHITE.gamma_multiply(0.28)
    };
    // Level from the label for a short way, then straight to the spot.
    let turn = pos2(edge.x - side * 14.0, edge.y);
    ui.painter()
        .add(Shape::line(vec![edge, turn, spot], Stroke::new(1.0, line)));
    ui.painter().circle_filled(spot, 2.4, line);
    theme::tag(ui.painter(), rect, text, selected, response.hovered(), ink);
    response.clicked()
}

/// The lighting preview: the mouse, placed and sized by `place`, with its
/// three zones shining through the lattice in `lights`. Returns a point on
/// the mouse's right edge beside each zone, for labels to attach to.
pub fn lighting(ui: &mut Ui, place: Place, lights: [Color32; 3]) -> [Pos2; 3] {
    let body = Body::placed(place.center, place.scale * LIGHTING_HEIGHT);
    remember_focus(ui, &body);
    let lit = lights != [Color32::BLACK; 3];

    // Light spilling out from under the shell.
    if lit {
        for ((from, to), color) in ZONE_BANDS.into_iter().zip(lights) {
            let along = (from + to) / 2.0;
            let (left, right) = Body::reach(along);
            ui.painter().add(theme::halo(
                body.at((right - left) / 2.0, along),
                vec2(body.of((left + right) * 0.78), body.of((to - from) * 0.95)),
                color.gamma_multiply(0.5),
            ));
        }
    }
    body.paint(ui, lit.then_some(lights));
    body.paint_wheel(ui);
    body.paint_cpi(ui, RUBBER, EDGE);

    ZONE_BANDS.map(|(from, to)| body.right((from + to) / 2.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> Body {
        Body::placed(pos2(300.0, 200.0), BUTTONS_HEIGHT)
    }

    #[test]
    fn the_outline_runs_from_nose_to_tail_without_doubling_back() {
        assert_eq!(OUTLINE[0].0, 0.0);
        assert_eq!(OUTLINE[OUTLINE.len() - 1].0, 1.0);
        for pair in OUTLINE.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{pair:?}");
        }
        assert!(OUTLINE
            .iter()
            .all(|(_, left, right)| *left > 0.0 && *right > 0.0));
    }

    #[test]
    fn the_mouse_has_the_proportions_of_the_real_one() {
        let body = body();
        assert!((body.y(0.0) - 22.0).abs() < 0.01, "placed by its middle");
        assert!((body.y(1.0) - 378.0).abs() < 0.01);
        let widest = (0..=200)
            .map(|step| {
                let along = step as f32 / 200.0;
                body.right(along).x - body.thumb_edge(along).x
            })
            .fold(0.0f32, f32::max);
        // 128.8 mm long and 68.2 mm wide. The photograph the outline was
        // measured from looks down on a mouse whose nose and tail curve
        // away from the camera, so it comes out a few percent short.
        let ratio = body.length / widest;
        assert!(
            (ratio - 128.8 / 68.2).abs() < 0.08,
            "length / width is {ratio}"
        );
        // Widest near the tail, as the real one is.
        let (left, right) = Body::reach(0.7);
        let (front_left, front_right) = Body::reach(0.3);
        assert!(left > front_left && right > front_right);
    }

    #[test]
    fn edges_between_measured_points_are_interpolated() {
        let (left, right) = Body::reach(0.3);
        assert!((0.251..=0.252).contains(&left), "{left}");
        assert!((0.243..=0.246).contains(&right), "{right}");
        assert_eq!(Body::reach(-1.0), Body::reach(0.0));
        assert_eq!(Body::reach(2.0), Body::reach(1.0));
    }

    #[test]
    fn the_lattice_stays_inside_the_shell_and_off_the_solid_parts() {
        let body = body();
        let openings = body.lattice();
        assert!(openings.len() > 100, "{} openings", openings.len());
        let reach = body.of(Body::opening_reach());
        let (wheel, cpi) = (body.wheel(), body.cpi());
        for opening in &openings {
            for (dx, dy) in [(reach, 0.0), (-reach, 0.0), (0.0, reach), (0.0, -reach)] {
                let corner = *opening + vec2(dx, dy);
                let along = body.along(corner.y);
                let (left, right) = (body.left(along).x, body.right(along).x);
                assert!(
                    corner.x > left + 3.0 && corner.x < right - 3.0,
                    "an opening at {opening:?} reaches the edge"
                );
                assert!(!wheel.contains(corner) && !cpi.contains(corner));
            }
        }
        // On the back of both main buttons, and all over the palm.
        let count = |from: f32, to: f32, side: f32| {
            openings
                .iter()
                .filter(|o| {
                    (from..to).contains(&body.along(o.y)) && (o.x - body.nose.x) * side > 0.0
                })
                .count()
        };
        assert!(count(BUTTON_LATTICE, SEAM, -1.0) >= 6);
        assert!(count(BUTTON_LATTICE, SEAM, 1.0) >= 6);
        assert!(count(0.45, 0.97, -1.0) > 30);
        assert!(count(0.45, 0.97, 1.0) > 30);
    }

    #[test]
    fn the_main_buttons_are_in_front_of_the_seam_and_clear_of_the_wheel() {
        let body = body();
        let wheel = body.wheel();
        for side in [-1.0, 1.0] {
            let rows = body.main_button(side);
            assert!(rows.len() > 30);
            for (inner, outer) in rows {
                assert!((outer.x - inner.x) * side > 0.0, "a row runs outwards");
                assert!(!wheel.expand(1.0).contains(inner));
                let along = body.along(outer.y);
                assert!(along <= SEAM + SEAM_DIP + 0.001);
                let edge = body.nose.x + side * body.of(Body::edge(side, along));
                assert!(
                    (outer.x - edge) * side <= 0.0,
                    "a row stays inside the outline"
                );
            }
        }
    }

    #[test]
    fn every_control_but_the_thumb_buttons_has_a_label_that_points_at_the_mouse() {
        for callout in &CALLOUTS {
            let (across, along) = callout.spot;
            let (left, right) = Body::reach(along);
            assert!(across > -left && across < right, "{:?}", callout.button);
            assert!((0.0..1.0).contains(&along));
        }
        let named: Vec<Button> = CALLOUTS.iter().map(|callout| callout.button).collect();
        for button in Button::ALL {
            assert_eq!(
                named.contains(&button),
                !matches!(button, Button::Grid(_)),
                "{button:?}"
            );
        }
    }

    #[test]
    fn light_mixes_between_zones_and_is_pure_within_them() {
        let lights = [Color32::RED, Color32::GREEN, Color32::BLUE];
        assert_eq!(zone_color(lights, 0.0), Color32::RED);
        assert_eq!(zone_color(lights, 0.3), Color32::RED);
        assert_eq!(zone_color(lights, 0.5), Color32::GREEN);
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

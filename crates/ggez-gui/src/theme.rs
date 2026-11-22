//! How ggez looks: colors, type, spacing, and the handful of custom
//! controls the pages are built from.
//!
//! Surfaces are lit from above: they are a little lighter at the top than
//! at the bottom, catch a hairline of light on their upper edge and cast a
//! soft shadow. The accent color is kept for what matters on a page: the
//! main action, what is selected, and what the user has changed.

use std::ops::RangeInclusive;
use std::sync::Arc;

use eframe::egui::{
    self, epaint, pos2, vec2, Align2, Color32, CornerRadius, FontFamily, FontId, Mesh, Painter,
    Pos2, Rect, Response, RichText, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, WidgetInfo,
    WidgetType,
};

pub const BG: Color32 = Color32::from_rgb(9, 10, 12);
pub const BG_TOP: Color32 = Color32::from_rgb(16, 18, 22);
pub const CARD: Color32 = Color32::from_rgb(21, 23, 28);
pub const CARD_TOP: Color32 = Color32::from_rgb(28, 30, 36);
pub const RAISED: Color32 = Color32::from_rgb(36, 39, 46);
pub const RAISED_HOVER: Color32 = Color32::from_rgb(47, 51, 60);
/// The floor of a track or a well: darker than the surface it is cut into.
pub const SUNKEN: Color32 = Color32::from_rgb(13, 14, 17);
pub const LINE: Color32 = Color32::from_rgb(40, 43, 51);
/// The edge of a raised control.
pub const EDGE: Color32 = Color32::from_rgb(54, 58, 68);
pub const TEXT: Color32 = Color32::from_rgb(238, 240, 245);
pub const TEXT_DIM: Color32 = Color32::from_rgb(158, 165, 179);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(108, 115, 130);
pub const ACCENT: Color32 = Color32::from_rgb(45, 212, 191);
/// The far end of the accent gradient.
pub const ACCENT_COOL: Color32 = Color32::from_rgb(56, 189, 248);
pub const ACCENT_DEEP: Color32 = Color32::from_rgb(19, 78, 74);
pub const ON_ACCENT: Color32 = Color32::from_rgb(3, 24, 26);
/// What is selected, and the main action: plain white, so that the only
/// colors in the window are the ones the mouse itself is showing.
pub const SELECT: Color32 = Color32::from_rgb(244, 245, 248);
pub const ON_SELECT: Color32 = Color32::from_rgb(10, 11, 14);
pub const WARNING: Color32 = Color32::from_rgb(251, 191, 36);
pub const DANGER: Color32 = Color32::from_rgb(248, 113, 113);
pub const GOOD: Color32 = Color32::from_rgb(74, 222, 128);

pub const CARD_RADIUS: u8 = 14;
pub const CONTROL_RADIUS: u8 = 8;
/// Pages do not stretch wider than this; very wide lines are hard to read.
pub const PAGE_WIDTH: f32 = 760.0;

/// The heavier face used for titles and emphasis.
pub fn strong() -> FontFamily {
    FontFamily::Name("strong".into())
}

/// Use the operating system's own interface font where there is one, so
/// the window looks at home; fall back to the built-in font otherwise.
fn install_fonts(ctx: &egui::Context) {
    const REGULAR: [&str; 5] = [
        r"C:\Windows\Fonts\segoeui.ttf",
        "/System/Library/Fonts/SFNS.ttf",
        "/System/Library/Fonts/Helvetica.ttc",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
    ];
    const STRONG: [&str; 4] = [
        r"C:\Windows\Fonts\seguisb.ttf",
        "/System/Library/Fonts/SFNS.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
    ];
    let load = |paths: &[&str]| paths.iter().find_map(|path| std::fs::read(path).ok());

    let mut fonts = egui::FontDefinitions::default();
    let defaults = fonts.families[&FontFamily::Proportional].clone();
    if let Some(bytes) = load(&REGULAR) {
        fonts
            .font_data
            .insert("system".into(), Arc::new(egui::FontData::from_owned(bytes)));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "system".into());
    }
    let mut strong_family = fonts.families[&FontFamily::Proportional].clone();
    if let Some(bytes) = load(&STRONG) {
        fonts.font_data.insert(
            "system-strong".into(),
            Arc::new(egui::FontData::from_owned(bytes)),
        );
        strong_family.insert(0, "system-strong".into());
    }
    // Keep the built-in fonts last for symbols the system font lacks.
    for name in defaults {
        if !strong_family.contains(&name) {
            strong_family.push(name);
        }
    }
    fonts.families.insert(strong(), strong_family);
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut(|style| {
        use egui::TextStyle;
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(12.0)),
            (TextStyle::Body, FontId::proportional(14.5)),
            (TextStyle::Button, FontId::proportional(14.5)),
            (TextStyle::Heading, FontId::new(24.0, strong())),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();

        let spacing = &mut style.spacing;
        spacing.item_spacing = vec2(10.0, 10.0);
        spacing.button_padding = vec2(14.0, 7.0);
        spacing.interact_size = vec2(40.0, 32.0);
        spacing.slider_width = 300.0;
        spacing.slider_rail_height = 6.0;
        spacing.combo_width = 220.0;
        spacing.menu_margin = egui::Margin::same(8);
        spacing.window_margin = egui::Margin::same(22);

        let visuals = &mut style.visuals;
        visuals.panel_fill = BG;
        visuals.window_fill = CARD_TOP;
        visuals.window_stroke = Stroke::new(1.0, EDGE);
        visuals.window_corner_radius = CornerRadius::same(CARD_RADIUS);
        visuals.menu_corner_radius = CornerRadius::same(10);
        visuals.extreme_bg_color = SUNKEN;
        visuals.faint_bg_color = CARD;
        visuals.hyperlink_color = ACCENT;
        // Neutral, like every other mark of what is selected. It is also
        // behind selected text, so it cannot be as light as [`SELECT`].
        visuals.selection.bg_fill = Color32::from_rgb(82, 88, 102);
        visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        visuals.slider_trailing_fill = true;
        visuals.popup_shadow = egui::Shadow {
            offset: [0, 12],
            blur: 32,
            spread: 0,
            color: Color32::from_black_alpha(150),
        };
        visuals.window_shadow = visuals.popup_shadow;

        let radius = CornerRadius::same(CONTROL_RADIUS);
        let widgets = &mut visuals.widgets;
        widgets.noninteractive.bg_fill = CARD;
        widgets.noninteractive.weak_bg_fill = CARD;
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        widgets.noninteractive.corner_radius = radius;
        for (state, fill, edge) in [
            (&mut widgets.inactive, RAISED, EDGE),
            (
                &mut widgets.hovered,
                RAISED_HOVER,
                lerp(EDGE, TEXT_FAINT, 0.5),
            ),
            (&mut widgets.active, RAISED_HOVER, TEXT_FAINT),
            (&mut widgets.open, RAISED_HOVER, TEXT_FAINT),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = Stroke::new(1.0, edge);
            state.fg_stroke = Stroke::new(1.0, TEXT);
            state.corner_radius = radius;
            state.expansion = 0.0;
        }
    });
}

// ---------------------------------------------------------------------
// Paint: gradients, glows and shadows
// ---------------------------------------------------------------------

/// Which way a gradient runs.
#[derive(Clone, Copy)]
pub enum Along {
    Down,
    Across,
}

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    let (a, b) = (a.to_array(), b.to_array());
    Color32::from_rgba_premultiplied(
        mix(a[0], b[0]),
        mix(a[1], b[1]),
        mix(a[2], b[2]),
        mix(a[3], b[3]),
    )
}

/// The outline of a rectangle with rounded corners, clockwise.
fn rounded_outline(rect: Rect, radius: f32) -> Vec<Pos2> {
    const STEPS: usize = 6;
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let corners = [
        (pos2(rect.right() - r, rect.top() + r), -90.0f32),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), 90.0),
        (pos2(rect.left() + r, rect.top() + r), 180.0),
    ];
    let mut points = Vec::with_capacity(4 * (STEPS + 1));
    for (centre, start) in corners {
        for step in 0..=STEPS {
            let angle = (start + 90.0 * step as f32 / STEPS as f32).to_radians();
            points.push(centre + r * vec2(angle.cos(), angle.sin()));
        }
    }
    points
}

/// A convex outline filled with colors that differ from point to point,
/// meeting at `hub`. The painter cannot smooth the edge of such a fill, so
/// callers draw a line over it.
pub fn fan(hub: (Pos2, Color32), rim: &[(Pos2, Color32)]) -> Shape {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(hub.0, hub.1);
    for (point, color) in rim {
        mesh.colored_vertex(*point, *color);
    }
    let count = rim.len() as u32;
    for i in 0..count {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % count);
    }
    Shape::mesh(mesh)
}

/// A rounded rectangle that fades from one color to another.
pub fn gradient(rect: Rect, radius: f32, from: Color32, to: Color32, along: Along) -> Shape {
    let at = |point: Pos2| match along {
        Along::Down => (point.y - rect.top()) / rect.height().max(1.0),
        Along::Across => (point.x - rect.left()) / rect.width().max(1.0),
    };
    let rim: Vec<(Pos2, Color32)> = rounded_outline(rect, radius)
        .into_iter()
        .map(|point| (point, lerp(from, to, at(point))))
        .collect();
    fan((rect.center(), lerp(from, to, 0.5)), &rim)
}

/// A soft-edged patch of color: light spilling from something, or with a
/// dark color the shadow under it.
pub fn glow(rect: Rect, radius: f32, color: Color32, blur: f32) -> Shape {
    epaint::RectShape::filled(rect, radius.min(120.0) as u8, color)
        .with_blur_width(blur)
        .into()
}

/// An ellipse of color that fades to nothing at its edge.
pub fn halo(centre: Pos2, size: Vec2, color: Color32) -> Shape {
    const STEPS: usize = 48;
    let rim: Vec<(Pos2, Color32)> = (0..STEPS)
        .map(|step| {
            let angle = step as f32 / STEPS as f32 * std::f32::consts::TAU;
            (
                centre + vec2(angle.cos() * size.x, angle.sin() * size.y),
                Color32::TRANSPARENT,
            )
        })
        .collect();
    fan((centre, color), &rim)
}

/// A raised panel: its shadow, a fill lit from above, and an edge.
pub fn surface(rect: Rect, radius: u8) -> Shape {
    let r = f32::from(radius);
    Shape::Vec(vec![
        glow(
            rect.translate(vec2(0.0, 8.0)).shrink(4.0),
            r,
            Color32::from_black_alpha(90),
            28.0,
        ),
        gradient(
            rect.shrink(0.5),
            r,
            CARD_TOP.gamma_multiply(0.9),
            CARD.gamma_multiply(0.9),
            Along::Down,
        ),
        Shape::rect_stroke(
            rect,
            CornerRadius::same(radius),
            Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.07)),
            StrokeKind::Inside,
        ),
    ])
}

/// A small raised control, such as a key in the drawing of the mouse.
pub fn raised(rect: Rect, radius: u8, fill: Color32, edge: Color32) -> Shape {
    let r = f32::from(radius);
    Shape::Vec(vec![
        glow(
            rect.translate(vec2(0.0, 2.0)),
            r,
            Color32::from_black_alpha(70),
            6.0,
        ),
        gradient(
            rect.shrink(0.5),
            r,
            lerp(fill, Color32::WHITE, 0.05),
            lerp(fill, Color32::BLACK, 0.12),
            Along::Down,
        ),
        Shape::rect_stroke(
            rect,
            CornerRadius::same(radius),
            Stroke::new(1.0, edge),
            StrokeKind::Inside,
        ),
    ])
}

/// What is behind everything: a dark stage, lit by the mouse's own lights.
/// `focus` is where the drawing of the mouse is and half its height; with
/// no drawing on the page the light comes down from above instead.
pub fn backdrop(painter: &Painter, rect: Rect, focus: Option<(Pos2, f32)>, lights: [Color32; 3]) {
    painter.add(gradient(rect, 0.0, BG_TOP, BG, Along::Down));
    match focus {
        Some((centre, half_height)) => {
            for (color, offset) in lights.into_iter().zip([-0.62, 0.0, 0.62]) {
                painter.add(halo(
                    centre + vec2(0.0, offset * half_height),
                    vec2(460.0, half_height * 0.95),
                    color.gamma_multiply(0.27),
                ));
            }
        }
        None => {
            for (color, across) in lights.into_iter().zip([0.2, 0.5, 0.8]) {
                painter.add(halo(
                    pos2(rect.left() + rect.width() * across, rect.top() - 40.0),
                    vec2(rect.width() * 0.34, 330.0),
                    color.gamma_multiply(0.13),
                ));
            }
        }
    }
}

// ---------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------

/// Outline a control that has keyboard focus, so someone moving through
/// the window with Tab can see where they are.
pub fn focus_ring(ui: &Ui, response: &Response, rect: Rect, radius: u8) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            CornerRadius::same(radius.saturating_add(2)),
            Stroke::new(1.5, TEXT),
            StrokeKind::Outside,
        );
    }
}

/// A rounded panel that groups related controls.
pub fn card<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    panel(ui, 20, add_contents)
}

/// A card with a chosen distance between its edge and its contents.
pub fn panel<R>(ui: &mut Ui, margin: i8, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    // The panel is painted once its size is known, underneath its contents.
    let behind = ui.painter().add(Shape::Noop);
    let shown = egui::Frame::new()
        .inner_margin(egui::Margin::same(margin))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui)
        });
    ui.painter()
        .set(behind, surface(shown.response.rect, CARD_RADIUS));
    shown.inner
}

/// The title of a card.
pub fn card_title(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(RichText::new(title).font(FontId::new(16.0, strong())));
        if !subtitle.is_empty() {
            ui.label(RichText::new(subtitle).color(TEXT_DIM).size(13.0));
        }
    });
    ui.add_space(6.0);
}

/// The small caption over a control or a group of them.
pub fn caption(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .font(FontId::new(11.5, strong()))
            .color(TEXT_FAINT),
    );
}

/// A remark the user should not miss: a tinted panel with a bar of the
/// given color down its left edge.
pub fn callout(ui: &mut Ui, color: Color32, text: &str) {
    let behind = ui.painter().add(Shape::Noop);
    let shown = egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 18,
            right: 14,
            top: 10,
            bottom: 10,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(text)
                    .color(lerp(color, TEXT, 0.72))
                    .size(13.5),
            );
        });
    let rect = shown.response.rect;
    let radius = CornerRadius::same(CONTROL_RADIUS);
    let bar = Rect::from_min_max(
        rect.left_top() + vec2(7.0, 9.0),
        rect.left_bottom() + vec2(10.0, -9.0),
    );
    ui.painter().set(
        behind,
        Shape::Vec(vec![
            Shape::rect_filled(rect, radius, lerp(CARD, color, 0.07)),
            Shape::rect_stroke(
                rect,
                radius,
                Stroke::new(1.0, lerp(LINE, color, 0.18)),
                StrokeKind::Inside,
            ),
            Shape::rect_filled(bar, CornerRadius::same(2), color),
        ]),
    );
}

fn text_width(ui: &Ui, text: &str, font: &FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font.clone(), TEXT)
        .size()
        .x
}

// ---------------------------------------------------------------------
// Controls
// ---------------------------------------------------------------------

/// How wide [`primary_button`] will be, for laying out around it.
pub fn primary_width(ui: &Ui, text: &str) -> f32 {
    text_width(ui, text, &FontId::new(14.5, strong())) + 56.0
}

/// The main action on a screen: a white pill that lights what is under it.
pub fn primary_button(ui: &mut Ui, enabled: bool, text: &str) -> Response {
    let font = FontId::new(14.5, strong());
    let size = vec2(primary_width(ui, text), 42.0);
    // A disabled button senses nothing, so it cannot report a click.
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, text));
    focus_ring(ui, &response, rect, 21);
    let painter = ui.painter();
    let radius = CornerRadius::same(21);
    if enabled {
        let hot = response.hovered();
        let pressed = response.is_pointer_button_down_on();
        painter.add(glow(
            rect.translate(vec2(0.0, 5.0)).shrink2(vec2(12.0, 6.0)),
            21.0,
            Color32::WHITE.gamma_multiply(if hot { 0.34 } else { 0.2 }),
            26.0,
        ));
        let fill = match (pressed, hot) {
            (true, _) => lerp(SELECT, Color32::BLACK, 0.12),
            (false, true) => Color32::WHITE,
            (false, false) => SELECT,
        };
        painter.rect_filled(rect, radius, fill);
        painter.text(rect.center(), Align2::CENTER_CENTER, text, font, ON_SELECT);
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        painter.rect_filled(rect, radius, Color32::WHITE.gamma_multiply(0.05));
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.08)),
            StrokeKind::Inside,
        );
        painter.text(rect.center(), Align2::CENTER_CENTER, text, font, TEXT_FAINT);
        response
    }
}

/// The pages of the window as a row of tabs in one pill. Returns the tab
/// that was clicked, if any.
pub fn tabs<T: Copy + PartialEq>(ui: &mut Ui, current: T, options: &[(T, &str)]) -> Option<T> {
    let font = FontId::proportional(14.0);
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, label)| text_width(ui, label, &font) + 32.0)
        .collect();
    let size = vec2(widths.iter().sum::<f32>() + 8.0, 40.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(20),
        Color32::WHITE.gamma_multiply(0.05),
    );
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(20),
        Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.08)),
        StrokeKind::Inside,
    );

    let mut clicked = None;
    let mut x = rect.left() + 4.0;
    for ((value, label), width) in options.iter().zip(widths) {
        let cell = Rect::from_min_size(pos2(x, rect.top() + 4.0), vec2(width, 32.0));
        x += width;
        let selected = *value == current;
        let response = ui.interact(cell, ui.id().with(("tab", *label)), Sense::click());
        response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, *label));
        focus_ring(ui, &response, cell, 16);
        let (ink, font) = if selected {
            ui.painter()
                .rect_filled(cell, CornerRadius::same(16), SELECT);
            (ON_SELECT, FontId::new(14.0, strong()))
        } else if response.hovered() {
            ui.painter().rect_filled(
                cell,
                CornerRadius::same(16),
                Color32::WHITE.gamma_multiply(0.07),
            );
            (TEXT, font.clone())
        } else {
            (TEXT_DIM, font.clone())
        };
        ui.painter()
            .text(cell.center(), Align2::CENTER_CENTER, *label, font, ink);
        if response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            clicked = Some(*value);
        }
    }
    clicked
}

/// A label attached to a part of the drawing of the mouse: a pill of text,
/// white when what it names is selected.
pub fn tag(painter: &Painter, rect: Rect, text: &str, selected: bool, hovered: bool, ink: Color32) {
    let radius = CornerRadius::same((rect.height() / 2.0) as u8);
    if selected {
        painter.rect_filled(rect, radius, SELECT);
    } else {
        painter.rect_filled(
            rect,
            radius,
            lerp(SUNKEN, RAISED, if hovered { 1.0 } else { 0.35 }).gamma_multiply(0.92),
        );
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(
                1.0,
                Color32::WHITE.gamma_multiply(if hovered { 0.3 } else { 0.14 }),
            ),
            StrokeKind::Inside,
        );
    }
    let font = if selected {
        FontId::new(12.5, strong())
    } else {
        FontId::proportional(12.5)
    };
    let ink = if selected { ON_SELECT } else { ink };
    let galley = painter.layout_no_wrap(text.to_owned(), font, ink);
    let at = pos2(
        (rect.center().x - galley.size().x / 2.0).max(rect.left() + 10.0),
        rect.center().y - galley.size().y / 2.0,
    );
    painter
        .with_clip_rect(rect.shrink2(vec2(8.0, 0.0)))
        .galley(at, galley, ink);
}

/// One of several equally weighted actions.
pub fn button(ui: &mut Ui, text: &str) -> Response {
    ui.add(egui::Button::new(text).min_size(vec2(0.0, 34.0)))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A row of mutually exclusive options joined into one control. Returns
/// the option that was clicked, if any.
pub fn segmented<T: Copy + PartialEq>(ui: &mut Ui, current: T, options: &[(T, &str)]) -> Option<T> {
    let font = FontId::proportional(14.0);
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, label)| (text_width(ui, label, &font) + 34.0).max(72.0))
        .collect();
    let size = vec2(widths.iter().sum::<f32>() + 8.0, 40.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    // A well that the chosen option sits in, raised.
    ui.painter()
        .rect_filled(rect, CornerRadius::same(11), SUNKEN);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(11),
        Stroke::new(1.0, LINE),
        StrokeKind::Inside,
    );

    let mut clicked = None;
    let mut x = rect.left() + 4.0;
    for ((value, label), width) in options.iter().zip(widths) {
        let cell = Rect::from_min_size(pos2(x, rect.top() + 4.0), vec2(width, 32.0));
        x += width;
        let selected = *value == current;
        let response = ui.interact(cell, ui.id().with(("segment", *label)), Sense::click());
        response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, *label));
        focus_ring(ui, &response, cell, CONTROL_RADIUS);
        let ink = if selected {
            ui.painter()
                .add(raised(cell, CONTROL_RADIUS, RAISED_HOVER, EDGE));
            TEXT
        } else if response.hovered() {
            ui.painter()
                .rect_filled(cell, CornerRadius::same(CONTROL_RADIUS), CARD_TOP);
            TEXT
        } else {
            TEXT_DIM
        };
        let font = if selected {
            FontId::new(14.0, strong())
        } else {
            font.clone()
        };
        ui.painter()
            .text(cell.center(), Align2::CENTER_CENTER, *label, font, ink);
        if response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            clicked = Some(*value);
        }
    }
    clicked
}

/// An on/off switch with its label, which wraps if there is not room for
/// it on one line. Returns whether the switch was flipped.
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str) -> bool {
    let font = FontId::proportional(14.5);
    let galley = ui.painter().layout(
        label.to_owned(),
        font,
        TEXT,
        (ui.available_width() - 54.0).max(80.0),
    );
    let size = vec2(54.0 + galley.size().x, galley.size().y.max(26.0));
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
    }
    let state = *on;
    response.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, state, label));

    let track = Rect::from_min_size(rect.left_top() + vec2(0.0, 1.0), vec2(42.0, 24.0));
    focus_ring(ui, &response, track, 12);
    let t = ui.ctx().animate_bool_with_time(response.id, *on, 0.14);
    let painter = ui.painter();
    painter.rect_filled(track, CornerRadius::same(12), SUNKEN);
    painter.rect_stroke(
        track,
        CornerRadius::same(12),
        Stroke::new(1.0, if response.hovered() { TEXT_FAINT } else { EDGE }),
        StrokeKind::Inside,
    );
    if t > 0.0 {
        painter.add(gradient(
            track,
            12.0,
            ACCENT.gamma_multiply(t),
            ACCENT_COOL.gamma_multiply(t),
            Along::Across,
        ));
        painter.rect_stroke(
            track,
            CornerRadius::same(12),
            Stroke::new(1.0, lerp(ACCENT, ACCENT_COOL, 0.5).gamma_multiply(t)),
            StrokeKind::Inside,
        );
    }
    let knob = pos2(
        egui::lerp(track.left() + 12.0..=track.right() - 12.0, t),
        track.center().y,
    );
    painter.add(glow(
        Rect::from_center_size(knob + vec2(0.0, 1.5), Vec2::splat(16.0)),
        8.0,
        Color32::from_black_alpha(110),
        5.0,
    ));
    painter.circle_filled(knob, 8.5, lerp(TEXT_DIM, Color32::WHITE, t));
    painter.galley(
        pos2(
            track.right() + 12.0,
            rect.top() + (26.0 - galley.rows[0].height()).max(0.0) / 2.0,
        ),
        galley,
        TEXT,
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// A slider over whole numbers. `step` gives the distance between values
/// the slider stops at, near a given value; `curved` spreads the low end
/// of the range out, for ranges where small values matter most.
pub fn slider(
    ui: &mut Ui,
    value: &mut u32,
    range: RangeInclusive<u32>,
    step: impl Fn(u32) -> u32,
    curved: bool,
    label: &str,
) -> Response {
    let width = ui.spacing().slider_width;
    let (rect, mut response) = ui.allocate_exact_size(vec2(width, 28.0), Sense::click_and_drag());
    let (low, high) = (*range.start(), *range.end());
    let rail = rect.left() + 10.0..=rect.right() - 10.0;
    let position = Travel { low, high, curved };

    let before = *value;
    if let Some(pointer) = response.interact_pointer_pos() {
        let t = egui::remap_clamp(pointer.x, rail.clone(), 0.0..=1.0);
        let raw = position.value_at(t);
        let step = step(raw.round() as u32).max(1) as f32;
        *value = (((raw / step).round() * step) as u32).clamp(low, high);
    }
    if response.has_focus() {
        // Keep the arrow keys for the slider instead of moving the focus.
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    ..Default::default()
                },
            );
        });
        let (down, up) = ui.input(|input| {
            (
                input.num_presses(egui::Key::ArrowLeft) + input.num_presses(egui::Key::ArrowDown),
                input.num_presses(egui::Key::ArrowRight) + input.num_presses(egui::Key::ArrowUp),
            )
        });
        for _ in 0..up {
            *value = value.saturating_add(step(*value).max(1)).min(high);
        }
        for _ in 0..down {
            // Stepping down from a boundary uses the smaller step below it.
            let size = step(value.saturating_sub(1)).max(1);
            *value = value.saturating_sub(size).max(low);
        }
    }
    if *value != before {
        response.mark_changed();
    }
    let shown = *value;
    response.widget_info(|| WidgetInfo::slider(true, f64::from(shown), label));

    let y = rect.center().y;
    let knob = pos2(egui::lerp(rail.clone(), position.fraction_of(shown)), y);
    let track = Rect::from_min_max(
        pos2(rect.left() + 4.0, y - 2.5),
        pos2(rect.right() - 4.0, y + 2.5),
    );
    let active = response.hovered() || response.dragged() || response.has_focus();
    let painter = ui.painter();
    painter.rect_filled(track, CornerRadius::same(3), SUNKEN);
    painter.rect_stroke(
        track.expand(0.5),
        CornerRadius::same(3),
        Stroke::new(1.0, LINE),
        StrokeKind::Outside,
    );
    let filled = Rect::from_min_max(track.min, pos2(knob.x, track.max.y));
    if filled.width() > 1.0 {
        painter.add(gradient(
            filled,
            3.0,
            lerp(ACCENT_DEEP, ACCENT, 0.35),
            ACCENT,
            Along::Across,
        ));
    }
    if active {
        painter.circle_filled(knob, 15.0, ACCENT.gamma_multiply(0.14));
    }
    painter.add(glow(
        Rect::from_center_size(knob + vec2(0.0, 2.0), Vec2::splat(18.0)),
        9.0,
        Color32::from_black_alpha(130),
        7.0,
    ));
    painter.circle_filled(knob, 9.0, Color32::WHITE);
    painter.circle_filled(knob, 3.5, ACCENT);
    focus_ring(ui, &response, rect, CONTROL_RADIUS);
    response.on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
}

/// How a slider's values are spread along its length.
#[derive(Clone, Copy)]
struct Travel {
    low: u32,
    high: u32,
    curved: bool,
}

impl Travel {
    /// A curved range that starts at zero is shifted by one, since zero
    /// has no logarithm.
    fn shift(self) -> f32 {
        if self.low == 0 {
            1.0
        } else {
            0.0
        }
    }

    fn fraction_of(self, value: u32) -> f32 {
        let (low, high, value) = (self.low as f32, self.high as f32, value as f32);
        let t = if self.curved {
            let s = self.shift();
            ((value + s).ln() - (low + s).ln()) / ((high + s).ln() - (low + s).ln())
        } else {
            (value - low) / (high - low)
        };
        t.clamp(0.0, 1.0)
    }

    fn value_at(self, t: f32) -> f32 {
        let (low, high) = (self.low as f32, self.high as f32);
        if self.curved {
            let s = self.shift();
            ((low + s).ln() + t * ((high + s).ln() - (low + s).ln())).exp() - s
        } else {
            low + t * (high - low)
        }
    }
}

/// A small rounded label such as `Ctrl` in `Ctrl + V`.
pub fn keycap(ui: &mut Ui, text: &str) {
    let font = FontId::new(13.0, strong());
    let size = vec2((text_width(ui, text, &font) + 18.0).max(30.0), 28.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().add(raised(rect, 7, RAISED_HOVER, EDGE));
    ui.painter().text(
        rect.center() - vec2(0.0, 0.5),
        Align2::CENTER_CENTER,
        text,
        font,
        TEXT,
    );
}

/// A bar filled in proportion to a level, such as the battery's.
pub fn meter(ui: &mut Ui, height: f32, fraction: f32, color: Color32) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let radius = (height / 2.0) as u8;
    ui.painter()
        .rect_filled(rect, CornerRadius::same(radius), SUNKEN);
    ui.painter().rect_stroke(
        rect.expand(0.5),
        CornerRadius::same(radius),
        Stroke::new(1.0, LINE),
        StrokeKind::Outside,
    );
    let mut filled = rect;
    filled.set_width((rect.width() * fraction.clamp(0.0, 1.0)).max(height));
    ui.painter().add(glow(
        filled.translate(vec2(0.0, 2.0)),
        height / 2.0,
        color.gamma_multiply(0.3),
        10.0,
    ));
    ui.painter().add(gradient(
        filled,
        height / 2.0,
        lerp(color, Color32::BLACK, 0.3),
        color,
        Along::Across,
    ));
    response
}

/// The ggez mark: a ring around a dot, as on the program's icon.
pub fn logo(ui: &mut Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let centre = rect.center();
    let painter = ui.painter();
    painter.add(halo(
        centre,
        Vec2::splat(size * 0.95),
        ACCENT.gamma_multiply(0.3),
    ));
    painter.circle_stroke(centre, size * 0.36, Stroke::new(size * 0.15, ACCENT));
    painter.circle_filled(centre, size * 0.11, ACCENT);
}

/// A filled circle used as a status light.
pub fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    ui.painter().add(halo(
        rect.center(),
        Vec2::splat(11.0),
        color.gamma_multiply(0.45),
    ));
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

/// The mark on a control that opens a list.
pub fn chevron(painter: &Painter, centre: Pos2, color: Color32) {
    let points = vec![
        centre + vec2(-4.0, -2.0),
        centre + vec2(0.0, 2.0),
        centre + vec2(4.0, -2.0),
    ];
    painter.add(Shape::line(points, Stroke::new(1.6, color)));
}

/// A color the user can click to change. Returns whether it changed.
pub fn swatch(ui: &mut Ui, rgb: &mut [u8; 3], label: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(52.0, 36.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::ColorButton, true, label));
    focus_ring(ui, &response, rect, CONTROL_RADIUS);
    let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    let radius = CornerRadius::same(CONTROL_RADIUS);
    let painter = ui.painter();
    // The color lights up what is around it, as the mouse's own lights do.
    painter.add(glow(
        rect.shrink(4.0).translate(vec2(0.0, 3.0)),
        8.0,
        color.gamma_multiply(if response.hovered() { 0.55 } else { 0.32 }),
        16.0,
    ));
    painter.rect_filled(rect, radius, color);
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, Color32::from_white_alpha(60)),
        StrokeKind::Inside,
    );

    let popup = response.id.with("picker");
    if response.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(popup));
    }
    let mut changed = false;
    egui::popup_below_widget(
        ui,
        popup,
        &response,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(250.0);
            ui.spacing_mut().slider_width = 230.0;
            let mut picked = color;
            if egui::color_picker::color_picker_color32(
                ui,
                &mut picked,
                egui::color_picker::Alpha::Opaque,
            ) {
                *rgb = [picked.r(), picked.g(), picked.b()];
                changed = true;
            }
        },
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand);
    changed
}

/// A button that opens a list, with the current choice as its text.
pub fn dropdown_button(ui: &mut Ui, text: &str, width: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 34.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, text));
    focus_ring(ui, &response, rect, CONTROL_RADIUS);
    let (fill, edge) = if response.hovered() {
        (RAISED_HOVER, lerp(EDGE, TEXT_FAINT, 0.5))
    } else {
        (RAISED, EDGE)
    };
    ui.painter().add(raised(rect, CONTROL_RADIUS, fill, edge));
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::proportional(14.5), TEXT);
    let clip = Rect::from_min_max(rect.min, rect.max - vec2(30.0, 0.0));
    ui.painter().with_clip_rect(clip).galley(
        pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0),
        galley,
        TEXT,
    );
    chevron(
        ui.painter(),
        pos2(rect.right() - 17.0, rect.center().y),
        TEXT_DIM,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slider_position_maps_back_to_its_value() {
        for travel in [
            Travel {
                low: 100,
                high: 18_000,
                curved: true,
            },
            Travel {
                low: 0,
                high: 1200,
                curved: true,
            },
            Travel {
                low: 0,
                high: 20,
                curved: false,
            },
        ] {
            for value in [
                travel.low,
                travel.low + 1,
                (travel.low + travel.high) / 2,
                travel.high,
            ] {
                let back = travel.value_at(travel.fraction_of(value));
                assert!(
                    (back - value as f32).abs() < 0.01 * value.max(1) as f32,
                    "{value} came back as {back}"
                );
            }
            assert_eq!(travel.fraction_of(travel.low), 0.0);
            assert_eq!(travel.fraction_of(travel.high), 1.0);
        }
    }

    #[test]
    fn a_rounded_outline_stays_inside_its_rectangle() {
        let rect = Rect::from_min_size(pos2(10.0, 20.0), vec2(120.0, 40.0));
        for radius in [0.0, 8.0, 100.0] {
            let points = rounded_outline(rect, radius);
            assert!(points.iter().all(|p| rect.expand(0.01).contains(*p)));
        }
    }
}

//! How ggez looks: colors, type, spacing, and the handful of custom
//! controls the pages are built from.

use std::sync::Arc;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontFamily, FontId, Rect, Response, RichText, Sense,
    Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType,
};

pub const BG: Color32 = Color32::from_rgb(13, 15, 19);
pub const SIDEBAR: Color32 = Color32::from_rgb(17, 20, 25);
pub const CARD: Color32 = Color32::from_rgb(24, 28, 35);
pub const RAISED: Color32 = Color32::from_rgb(33, 38, 47);
pub const RAISED_HOVER: Color32 = Color32::from_rgb(42, 48, 59);
pub const LINE: Color32 = Color32::from_rgb(41, 47, 58);
pub const TEXT: Color32 = Color32::from_rgb(232, 235, 241);
pub const TEXT_DIM: Color32 = Color32::from_rgb(150, 159, 175);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(103, 112, 128);
pub const ACCENT: Color32 = Color32::from_rgb(45, 212, 191);
pub const ACCENT_DEEP: Color32 = Color32::from_rgb(17, 94, 86);
pub const ON_ACCENT: Color32 = Color32::from_rgb(4, 30, 27);
pub const WARNING: Color32 = Color32::from_rgb(251, 191, 36);
pub const DANGER: Color32 = Color32::from_rgb(248, 113, 113);
pub const GOOD: Color32 = Color32::from_rgb(74, 222, 128);

pub const CARD_RADIUS: u8 = 12;
pub const CONTROL_RADIUS: u8 = 8;
/// Pages do not stretch wider than this; very wide lines are hard to read.
pub const PAGE_WIDTH: f32 = 720.0;

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
        spacing.item_spacing = egui::vec2(10.0, 10.0);
        spacing.button_padding = egui::vec2(14.0, 7.0);
        spacing.interact_size = egui::vec2(40.0, 32.0);
        spacing.slider_width = 300.0;
        spacing.slider_rail_height = 6.0;
        spacing.combo_width = 220.0;
        spacing.menu_margin = egui::Margin::same(8);
        spacing.window_margin = egui::Margin::same(20);

        let visuals = &mut style.visuals;
        visuals.panel_fill = BG;
        visuals.window_fill = CARD;
        visuals.window_stroke = Stroke::new(1.0, LINE);
        visuals.window_corner_radius = CornerRadius::same(CARD_RADIUS);
        visuals.menu_corner_radius = CornerRadius::same(10);
        visuals.extreme_bg_color = Color32::from_rgb(9, 11, 14);
        visuals.faint_bg_color = CARD;
        visuals.hyperlink_color = ACCENT;
        visuals.selection.bg_fill = ACCENT_DEEP;
        visuals.selection.stroke = Stroke::new(1.0, ACCENT);
        visuals.slider_trailing_fill = true;
        visuals.popup_shadow = egui::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(120),
        };
        visuals.window_shadow = visuals.popup_shadow;

        let radius = CornerRadius::same(CONTROL_RADIUS);
        let widgets = &mut visuals.widgets;
        widgets.noninteractive.bg_fill = CARD;
        widgets.noninteractive.weak_bg_fill = CARD;
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        widgets.noninteractive.corner_radius = radius;
        for (state, fill, text) in [
            (&mut widgets.inactive, RAISED, TEXT),
            (&mut widgets.hovered, RAISED_HOVER, TEXT),
            (&mut widgets.active, RAISED_HOVER, TEXT),
            (&mut widgets.open, RAISED_HOVER, TEXT),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = Stroke::NONE;
            state.fg_stroke = Stroke::new(1.0, text);
            state.corner_radius = radius;
            state.expansion = 0.0;
        }
        widgets.hovered.bg_stroke = Stroke::new(1.0, LINE);
    });
}

/// A rounded panel that groups related controls.
pub fn card<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(egui::Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui)
        })
        .inner
}

/// The title at the top of a page.
pub fn page_title(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(title).font(FontId::new(26.0, strong())));
    ui.label(RichText::new(subtitle).color(TEXT_DIM));
    ui.add_space(10.0);
}

/// The title of a card.
pub fn card_title(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).font(FontId::new(16.0, strong())));
    if !subtitle.is_empty() {
        ui.label(RichText::new(subtitle).color(TEXT_DIM).size(13.0));
    }
    ui.add_space(4.0);
}

/// A boxed remark the user should not miss.
pub fn callout(ui: &mut Ui, color: Color32, text: &str) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.09))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.45)))
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).color(color).size(13.5));
        });
}

fn text_width(ui: &Ui, text: &str, font: &FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font.clone(), TEXT)
        .size()
        .x
}

/// The main action on a screen.
pub fn primary_button(ui: &mut Ui, enabled: bool, text: &str) -> Response {
    let font = FontId::new(14.5, strong());
    let size = egui::vec2(text_width(ui, text, &font) + 40.0, 38.0);
    // A disabled button senses nothing, so it cannot report a click.
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, text));
    let hot = enabled && response.hovered();
    let (fill, ink) = if !enabled {
        (RAISED, TEXT_FAINT)
    } else if hot {
        (lerp(ACCENT, Color32::WHITE, 0.18), ON_ACCENT)
    } else {
        (ACCENT, ON_ACCENT)
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), fill);
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, text, font, ink);
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// One of several equally weighted actions.
pub fn button(ui: &mut Ui, text: &str) -> Response {
    ui.add(egui::Button::new(text).min_size(egui::vec2(0.0, 34.0)))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A row of mutually exclusive options joined into one control. Returns
/// the option that was clicked, if any.
pub fn segmented<T: Copy + PartialEq>(ui: &mut Ui, current: T, options: &[(T, &str)]) -> Option<T> {
    let font = FontId::proportional(14.0);
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, label)| (text_width(ui, label, &font) + 30.0).max(64.0))
        .collect();
    let size = egui::vec2(widths.iter().sum::<f32>() + 6.0, 36.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(10), RAISED);

    let mut clicked = None;
    let mut x = rect.left() + 3.0;
    for ((value, label), width) in options.iter().zip(widths) {
        let cell = Rect::from_min_size(egui::pos2(x, rect.top() + 3.0), egui::vec2(width, 30.0));
        x += width;
        let selected = *value == current;
        let response = ui.interact(cell, ui.id().with(("segment", *label)), Sense::click());
        response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, *label));
        let ink = if selected {
            ui.painter()
                .rect_filled(cell, CornerRadius::same(CONTROL_RADIUS), ACCENT);
            ON_ACCENT
        } else if response.hovered() {
            ui.painter()
                .rect_filled(cell, CornerRadius::same(CONTROL_RADIUS), RAISED_HOVER);
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

/// An on/off switch with its label. Returns whether it was flipped.
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str) -> bool {
    let font = FontId::proportional(14.5);
    let size = egui::vec2(46.0 + 10.0 + text_width(ui, label, &font), 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
    }
    let state = *on;
    response.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, state, label));

    let track = Rect::from_min_size(rect.min, egui::vec2(46.0, 26.0));
    let t = ui.ctx().animate_bool_with_time(response.id, *on, 0.12);
    ui.painter()
        .rect_filled(track, CornerRadius::same(13), lerp(RAISED_HOVER, ACCENT, t));
    let knob_x = egui::lerp(track.left() + 13.0..=track.right() - 13.0, t);
    ui.painter().circle_filled(
        egui::pos2(knob_x, track.center().y),
        9.0,
        lerp(TEXT_DIM, Color32::WHITE, t),
    );
    ui.painter().text(
        egui::pos2(track.right() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        TEXT,
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// A small rounded label such as `Ctrl` in `Ctrl + V`.
pub fn keycap(ui: &mut Ui, text: &str) {
    let font = FontId::new(13.0, strong());
    let size = egui::vec2((text_width(ui, text, &font) + 16.0).max(28.0), 26.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(6), RAISED_HOVER);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(1.0, LINE),
        StrokeKind::Inside,
    );
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, text, font, TEXT);
}

/// A filled circle used as a status light.
pub fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.5, color);
    ui.painter()
        .circle_filled(rect.center(), 7.5, color.gamma_multiply(0.18));
}

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// A color the user can click to change. Returns whether it changed.
pub fn swatch(ui: &mut Ui, rgb: &mut [u8; 3], label: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(56.0, 36.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::ColorButton, true, label));
    let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    let radius = CornerRadius::same(CONTROL_RADIUS);
    ui.painter().rect_filled(rect, radius, color);
    let edge = if response.hovered() { TEXT } else { LINE };
    ui.painter()
        .rect_stroke(rect, radius, Stroke::new(1.5, edge), StrokeKind::Inside);

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
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, text));
    let fill = if response.hovered() {
        RAISED_HOVER
    } else {
        RAISED
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), fill);
    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(14.5),
        TEXT,
    );
    let tip = egui::pos2(rect.right() - 16.0, rect.center().y + 3.0);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            tip,
            tip + egui::vec2(-5.0, -6.0),
            tip + egui::vec2(5.0, -6.0),
        ],
        TEXT_DIM,
        Stroke::NONE,
    ));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

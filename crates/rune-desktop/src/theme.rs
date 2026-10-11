//! Rune's look: one warm dark palette and one amber accent, shared with the Android app.

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
    self, Align, Color32, CornerRadius, FontId, Margin, Response, RichText, Stroke, TextStyle, Ui,
    Vec2,
};
pub use egui_phosphor::regular as icon;

pub const BG: Color32 = Color32::from_rgb(0x0D, 0x0B, 0x09);
pub const SURFACE: Color32 = Color32::from_rgb(0x18, 0x15, 0x12);
pub const SURFACE_HI: Color32 = Color32::from_rgb(0x23, 0x1F, 0x1A);
pub const BORDER: Color32 = Color32::from_rgb(0x2C, 0x27, 0x22);
pub const TEXT: Color32 = Color32::from_rgb(0xF6, 0xF1, 0xE9);
pub const MUTED: Color32 = Color32::from_rgb(0xA8, 0x9F, 0x92);
pub const ACCENT: Color32 = Color32::from_rgb(0xFF, 0xB0, 0x20);
pub const ON_ACCENT: Color32 = Color32::from_rgb(0x1A, 0x12, 0x00);
pub const DANGER: Color32 = Color32::from_rgb(0xF8, 0x71, 0x71);

pub const RADIUS: u8 = 10;
/// Spacing rhythm: 4 / 8 / 12 / 16 / 24.
pub const GAP: f32 = 8.0;
pub const PAD: i8 = 16;

pub fn apply(ctx: &egui::Context) {
    // Space Grotesk for the interface, JetBrains Mono for passwords and keys (0/O and l/1 stay
    // distinct). Both are variable fonts, so the weight is chosen here: Space Grotesk's own
    // default is Light, which is too thin on a dark ground.
    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes, family, weight) in [
        ("space_grotesk", &include_bytes!("../../../assets/fonts/space_grotesk.ttf")[..], egui::FontFamily::Proportional, 450.0),
        ("jetbrains_mono", &include_bytes!("../../../assets/fonts/jetbrains_mono.ttf")[..], egui::FontFamily::Monospace, 450.0),
    ] {
        let tweak = egui::FontTweak {
            coords: egui::epaint::text::VariationCoords::new([(b"wght", weight)]),
            ..Default::default()
        };
        fonts.font_data.insert(name.to_owned(), egui::FontData::from_static(bytes).tweak(tweak).into());
        fonts.families.entry(family).or_default().insert(0, name.to_owned());
    }
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    // Icons are drawn from their own family (see `glyph`), never through the text fallback chain:
    // a text font may have private-use glyphs of its own, and the icon font has ligatures for
    // plain letters.
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(family).or_default().retain(|name| name != "phosphor");
    }
    fonts.families.insert(icon_family(), vec!["phosphor".to_owned()]);
    ctx.set_fonts(fonts);
    ctx.set_theme(egui::Theme::Dark);

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, FontId::proportional(22.0)),
            (TextStyle::Body, FontId::proportional(15.0)),
            (TextStyle::Button, FontId::proportional(15.0)),
            (TextStyle::Monospace, FontId::monospace(15.0)),
            (TextStyle::Small, FontId::proportional(13.0)),
        ]
        .into();
        style.spacing.item_spacing = Vec2::splat(GAP);
        style.spacing.button_padding = Vec2::new(14.0, 8.0);
        style.spacing.interact_size.y = 36.0;
        style.spacing.icon_width = 18.0;
        style.spacing.slider_width = 180.0;

        let v = &mut style.visuals;
        v.panel_fill = BG;
        v.window_fill = SURFACE;
        v.extreme_bg_color = SURFACE; // text field background
        v.faint_bg_color = SURFACE;
        v.override_text_color = Some(TEXT);
        v.selection.bg_fill = ACCENT.gamma_multiply(0.35);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.hyperlink_color = ACCENT;
        v.error_fg_color = DANGER;
        v.warn_fg_color = DANGER;

        let radius = CornerRadius::same(RADIUS);
        let w = &mut v.widgets;
        w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.noninteractive.corner_radius = radius;
        for (state, fill, stroke) in [
            (&mut w.inactive, SURFACE_HI, BORDER),
            (&mut w.hovered, BORDER, MUTED),
            (&mut w.active, BORDER, ACCENT),
            (&mut w.open, SURFACE_HI, ACCENT),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = Stroke::new(1.0, stroke);
            state.fg_stroke = Stroke::new(1.5, TEXT);
            state.corner_radius = radius;
            state.expansion = 0.0; // no layout jitter on hover
        }
        // Keyboard focus ring.
        w.hovered.bg_stroke = Stroke::new(1.0, MUTED);
        v.selection.stroke = Stroke::new(1.5, ACCENT);
    });
}

pub fn muted(text: impl Into<String>) -> RichText {
    RichText::new(text).color(MUTED)
}

pub fn small(text: impl Into<String>) -> RichText {
    RichText::new(text).color(MUTED).small()
}

fn icon_family() -> egui::FontFamily {
    egui::FontFamily::Name("phosphor".into())
}

/// An icon on its own, e.g. `ui.label(theme::glyph(icon::WARNING))`.
pub fn glyph(glyph: &str) -> RichText {
    RichText::new(glyph).family(icon_family())
}

/// Optional icon, then text, in one colour. Pass `""` for no icon.
pub fn icon_text(glyph: &str, text: &str, color: Color32, size: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let format = |font_id| TextFormat { font_id, color, valign: Align::Center, ..Default::default() };
    if !glyph.is_empty() {
        job.append(glyph, 0.0, format(FontId::new(size + 3.0, icon_family())));
    }
    if !text.is_empty() {
        let gap = if glyph.is_empty() { 0.0 } else { 8.0 };
        job.append(text, gap, format(FontId::proportional(size)));
    }
    job
}

/// The one filled, high-emphasis action on a screen.
pub fn primary_button(glyph: &str, text: &str) -> egui::Button<'static> {
    egui::Button::new(icon_text(glyph, text, ON_ACCENT, 15.0)).fill(ACCENT).stroke(Stroke::NONE)
}

pub fn button(glyph: &str, text: &str) -> egui::Button<'static> {
    egui::Button::new(icon_text(glyph, text, TEXT, 15.0))
}

pub fn danger_button(glyph: &str, text: &str) -> egui::Button<'static> {
    egui::Button::new(icon_text(glyph, text, DANGER, 15.0)).stroke(Stroke::new(1.0, DANGER.gamma_multiply(0.6)))
}

/// A frameless icon-only button. `label` is its tooltip and accessible name.
pub fn icon_button(ui: &mut Ui, icon: &str, label: &str) -> Response {
    let button = egui::Button::new(glyph(icon).size(20.0)).frame_when_inactive(false);
    let response = ui.add_sized([36.0, 36.0], button).on_hover_text(label);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response
}

/// A line of small text with an icon in front: notes, warnings, confirmations.
pub fn note(ui: &mut Ui, glyph: &str, text: &str, color: Color32) -> Response {
    ui.add(egui::Label::new(icon_text(glyph, text, color, 13.0)).wrap())
}

pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(RADIUS + 2))
        .inner_margin(Margin::same(PAD))
}

/// A labelled form field: small label above, widget below.
pub fn field(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> Response) -> Response {
    let label = ui.label(small(label));
    let response = add(ui).labelled_by(label.id);
    ui.add_space(4.0);
    response
}

pub fn text_field(text: &mut String) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text).desired_width(f32::INFINITY).margin(Margin::symmetric(12, 9))
}

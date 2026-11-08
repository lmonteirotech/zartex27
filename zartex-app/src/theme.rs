//! Look of the app: Windows 11 style (Fluent-like) in a light and a dark palette, with a configurable accent colour.
//! Colours are read through functions (`theme::text()`, ...) because they change with the theme.
//! Holds the Appearance choice, the Palette of each theme, the global palette that apply sets, the Windows fonts and
//! the Windows accent colour, and the small drawing helpers: the card frame and the logic node colours.
//! Uses egui only. Used by most UI files: main.rs, overview.rs, propedit.rs, view3d.rs, modelbox.rs, and settings.rs
//! for the Appearance type.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke};
use std::sync::{Arc, RwLock};

/// Which palette is used: follow the system, or always light, or always dark.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Appearance {
    /// Follow the system light or dark setting.
    System,
    /// Always the light palette.
    Light,
    /// Always the dark palette.
    Dark,
}

impl Appearance {
    /// Name stored in the settings file: system, light or dark.
    pub fn key(self) -> &'static str {
        match self {
            Appearance::System => "system",
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        }
    }
    /// Reads a name from the settings file (any case). None for any other text.
    pub fn parse(s: &str) -> Option<Appearance> {
        match s.trim().to_lowercase().as_str() {
            "system" => Some(Appearance::System),
            "light" => Some(Appearance::Light),
            "dark" => Some(Appearance::Dark),
            _ => None,
        }
    }
}

/// All colours of one theme: light or dark, with the accent colour.
#[derive(Clone, Copy)]
struct Palette {
    dark: bool,
    accent: Color32,
    accent_soft: Color32,
    window_bg: Color32,
    card_bg: Color32,
    card_stroke: Color32,
    text: Color32,
    text_dim: Color32,
    canvas_bg: Color32,
    grid: Color32,
    bar_bg: Color32,
    hover_bg: Color32,
    node_body: Color32,
    node_border: Color32,
}

/// Default accent colour: Windows Fluent blue, #0067C0.
const FLUENT_BLUE: [u8; 3] = [0x00, 0x67, 0xC0];

/// Converts an [r, g, b] array to a colour.
fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Blends two colours per channel: t = 0 gives a, t = 1 gives b (rounded).
fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t).round() as u8;
    Color32::from_rgb(f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2]))
}

/// Builds the palette for dark or light mode with this accent. On dark surfaces the accent is lightened so it stays readable; accent_soft is a dark tint in dark mode and a pale tint in light mode.
fn palette(dark: bool, accent: [u8; 3]) -> Palette {
    if dark {
        // the accent is lifted a little so that it reads on dark surfaces
        let acc = mix(accent, [255, 255, 255], 0.18);
        Palette {
            dark,
            accent: acc,
            accent_soft: mix(accent, [0x2B, 0x2B, 0x2B], 0.62),
            window_bg: Color32::from_rgb(0x20, 0x20, 0x20),
            card_bg: Color32::from_rgb(0x2B, 0x2B, 0x2B),
            card_stroke: Color32::from_rgb(0x3D, 0x3D, 0x3D),
            text: Color32::from_rgb(0xF2, 0xF2, 0xF2),
            text_dim: Color32::from_rgb(0xA9, 0xA9, 0xA9),
            canvas_bg: Color32::from_rgb(0x1A, 0x1A, 0x1A),
            grid: Color32::from_rgb(0x2E, 0x2E, 0x2E),
            bar_bg: Color32::from_rgb(0x18, 0x18, 0x18),
            hover_bg: Color32::from_rgb(0x38, 0x38, 0x38),
            node_body: Color32::from_rgb(0x2F, 0x2F, 0x33),
            node_border: Color32::from_rgb(0x50, 0x50, 0x55),
        }
    } else {
        Palette {
            dark,
            accent: rgb(accent),
            accent_soft: mix(accent, [255, 255, 255], 0.86),
            window_bg: Color32::from_rgb(0xF3, 0xF3, 0xF3),
            card_bg: Color32::from_rgb(0xFB, 0xFB, 0xFB),
            card_stroke: Color32::from_rgb(0xE3, 0xE3, 0xE3),
            text: Color32::from_rgb(0x1B, 0x1B, 0x1B),
            text_dim: Color32::from_rgb(0x5F, 0x5F, 0x5F),
            canvas_bg: Color32::from_rgb(0xF7, 0xF7, 0xF7),
            grid: Color32::from_rgb(0xDC, 0xDC, 0xDC),
            bar_bg: Color32::from_rgb(0xEE, 0xEE, 0xEE),
            hover_bg: Color32::from_rgb(0xDD, 0xDD, 0xDD),
            node_body: Color32::WHITE,
            node_border: Color32::from_rgb(0xD0, 0xD0, 0xD0),
        }
    }
}

/// The palette in use. apply writes it and the colour functions read it. Until apply runs, the light palette with the default accent is used.
static PAL: RwLock<Option<Palette>> = RwLock::new(None);

/// The palette in use, or the light default when none has been applied.
fn p() -> Palette {
    PAL.read().ok().and_then(|g| *g).unwrap_or_else(|| palette(false, FLUENT_BLUE))
}

/// Accent colour: buttons, selections and markers.
pub fn accent() -> Color32 {
    p().accent
}
/// Tint of the accent for selected backgrounds.
pub fn accent_soft() -> Color32 {
    p().accent_soft
}
/// Background of the window and panels.
pub fn window_bg() -> Color32 {
    p().window_bg
}
/// Background of cards and side panels.
pub fn card_bg() -> Color32 {
    p().card_bg
}
/// Thin border of cards and panels.
pub fn card_stroke() -> Color32 {
    p().card_stroke
}
/// Main text colour.
pub fn text() -> Color32 {
    p().text
}
/// Secondary text colour: labels and hints.
pub fn text_dim() -> Color32 {
    p().text_dim
}
/// Background of the logic graph and the 3D views.
pub fn canvas_bg() -> Color32 {
    p().canvas_bg
}
/// Dots of the logic graph background.
pub fn grid() -> Color32 {
    p().grid
}
/// Background of the title bar and the status line.
pub fn bar_bg() -> Color32 {
    p().bar_bg
}
/// Background of a hovered title bar button.
pub fn hover_bg() -> Color32 {
    p().hover_bg
}
/// Body of a logic node card in the graph.
pub fn node_body() -> Color32 {
    p().node_body
}
/// Border of a logic node card.
pub fn node_border() -> Color32 {
    p().node_border
}
/// True when the dark palette is in use.
pub fn is_dark() -> bool {
    p().dark
}

/// A faint wash for hovered flat widgets: dark on light themes, light on dark ones.
pub fn overlay(alpha: u8) -> Color32 {
    if p().dark {
        Color32::from_white_alpha(alpha.saturating_add(8))
    } else {
        Color32::from_black_alpha(alpha)
    }
}

/// Timeline segment colours (alternating) and the blend part of a segment.
pub fn seg(k: usize) -> Color32 {
    match (p().dark, k % 2) {
        (false, 0) => Color32::from_rgb(0xD9, 0xCC, 0xEE),
        (false, _) => Color32::from_rgb(0xC8, 0xB6, 0xE6),
        (true, 0) => Color32::from_rgb(0x4A, 0x3F, 0x66),
        (true, _) => Color32::from_rgb(0x57, 0x49, 0x7A),
    }
}
/// Colour of the blend part of a shot segment on the camera timeline.
pub fn seg_blend() -> Color32 {
    if p().dark {
        Color32::from_rgb(0x74, 0x5D, 0xA6)
    } else {
        Color32::from_rgb(0xA8, 0x8F, 0xD6)
    }
}

/// Loads the Windows fonts once (Segoe UI, Consolas); the visuals are applied by `apply`.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    // Segoe UI (regular and bold) and Consolas are used when installed; otherwise egui's own fonts are kept
    for (name, file) in [("segoe", "segoeui.ttf"), ("segoe-bold", "segoeuib.ttf"), ("mono", "consola.ttf")] {
        let path = format!("C:\\Windows\\Fonts\\{file}");
        if let Ok(bytes) = std::fs::read(&path) {
            fonts.font_data.insert(name.to_string(), Arc::new(FontData::from_owned(bytes)));
        }
    }
    if fonts.font_data.contains_key("segoe") {
        fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "segoe".into());
    }
    if fonts.font_data.contains_key("mono") {
        fonts.families.get_mut(&FontFamily::Monospace).unwrap().insert(0, "mono".into());
    }
    ctx.set_fonts(fonts);
}

/// Sets the palette (light or dark, with this accent) and the matching egui visuals.
pub fn apply(ctx: &egui::Context, dark: bool, accent: [u8; 3]) {
    // the palette is stored first, so the colour functions return the new values from now on
    let pal = palette(dark, accent);
    if let Ok(mut g) = PAL.write() {
        *g = Some(pal);
    }
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = pal.window_bg;
    v.window_fill = pal.card_bg;
    v.extreme_bg_color = if dark { Color32::from_rgb(0x1E, 0x1E, 0x1E) } else { Color32::WHITE };
    v.faint_bg_color = if dark { Color32::from_rgb(0x26, 0x26, 0x26) } else { Color32::from_rgb(0xF0, 0xF0, 0xF0) };
    v.override_text_color = Some(pal.text);
    v.selection.bg_fill = pal.accent_soft;
    v.selection.stroke = Stroke::new(1.0, pal.accent);
    v.hyperlink_color = pal.accent;
    // rounded corners of 6 px on every widget
    let r = CornerRadius::same(6);
    v.widgets.noninteractive.corner_radius = r;
    v.widgets.inactive.corner_radius = r;
    v.widgets.hovered.corner_radius = r;
    v.widgets.active.corner_radius = r;
    let (idle, hover) = if dark { (Color32::from_rgb(0x34, 0x34, 0x34), Color32::from_rgb(0x3E, 0x3E, 0x3E)) } else { (Color32::from_rgb(0xFD, 0xFD, 0xFD), Color32::from_rgb(0xF5, 0xF5, 0xF5)) };
    v.widgets.inactive.bg_fill = idle;
    v.widgets.inactive.weak_bg_fill = idle;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, pal.card_stroke);
    v.widgets.hovered.bg_fill = hover;
    v.widgets.hovered.weak_bg_fill = hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, if dark { Color32::from_rgb(0x5A, 0x5A, 0x5A) } else { Color32::from_rgb(0xCB, 0xCB, 0xCB) });
    v.widgets.active.bg_fill = pal.accent_soft;
    v.widgets.active.weak_bg_fill = pal.accent_soft;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, pal.card_stroke);
    ctx.set_visuals(v);

    // sizes and text styles (the fonts themselves come from install_fonts)
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 3.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.interact_size.y = 26.0;
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(13.5));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(13.5));
        s.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(18.0));
        s.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(11.5));
        s.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(12.5));
    });
}

/// The accent colour of Windows (Settings > Personalization > Colors), when this is Windows and it can be read.
#[cfg(windows)]
pub fn windows_accent() -> Option<[u8; 3]> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("reg").args(["query", r"HKCU\Software\Microsoft\Windows\DWM", "/v", "AccentColor"]).creation_flags(0x0800_0000).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let hex = text.split("0x").nth(1)?.split_whitespace().next()?;
    let v = u32::from_str_radix(hex.trim(), 16).ok()?;
    // stored as 0xAABBGGRR
    Some([(v & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, ((v >> 16) & 0xFF) as u8])
}

/// Accent colour of Windows on other systems: there is none, so None.
#[cfg(not(windows))]
pub fn windows_accent() -> Option<[u8; 3]> {
    None
}

/// Frame of a card: card background, thin border, 8 px corners and a 10 px inner margin.
pub fn card() -> egui::Frame {
    egui::Frame::new().fill(card_bg()).stroke(Stroke::new(1.0, card_stroke())).corner_radius(CornerRadius::same(8)).inner_margin(egui::Margin::same(10))
}

/// Category colour for a node header.
pub fn node_color(label: &str, type_id: u8) -> Color32 {
    let l = label.to_lowercase();
    if type_id == 1 || l.contains("start") {
        Color32::from_rgb(0x10, 0x7C, 0x10)
    } else if l.contains("fail") || l.contains("dead") || l.contains("wrecked") || l.contains("arrest") {
        Color32::from_rgb(0xC4, 0x2B, 0x1C)
    } else if l.contains("complete") || l.contains("success") {
        Color32::from_rgb(0x0F, 0x7B, 0x6C)
    } else if l.contains("timer") || l.contains("clock") || l.contains("counter") || l.contains("delay") {
        Color32::from_rgb(0xC1, 0x7A, 0x00)
    } else if l.contains("camera") || l.contains("audio") || l.contains("music") || l.contains("message") || l.contains("fade") {
        Color32::from_rgb(0x75, 0x4A, 0xB8)
    } else {
        accent()
    }
}

use eframe::egui::{self, vec2, Color32, Stroke};
use std::sync::Arc;

/// Colours of the original home window (home_dialog.css, Material 3 "gm3-sys-color-*").
#[derive(Clone, Copy)]
pub struct Palette {
    pub background: Color32,        // --gm3-sys-color-background
    pub container: Color32,         // --gm3-sys-color-surface-container (nav + top bar)
    pub container_low: Color32,     // --gm3-sys-color-surface-container-low
    pub container_high: Color32,    // --gm3-sys-color-surface-container-high (search box)
    pub on_surface: Color32,        // --gm3-sys-color-on-surface
    pub on_variant: Color32,        // --gm3-sys-color-on-surface-variant
    pub outline: Color32,           // --gm3-sys-color-outline-variant
    pub primary: Color32,           // --gm3-sys-color-primary
    pub primary_container: Color32, // --gm3-sys-color-primary-container
    pub selected: Color32,          // --gm3-sys-color-secondary-container
    pub on_selected: Color32,       // --gm3-sys-color-on-secondary-container
    pub on_primary: Color32,
    pub green: Color32,
    pub red: Color32,
}

pub const LIGHT: Palette = Palette {
    background: Color32::from_rgb(0xff, 0xff, 0xff),
    container: Color32::from_rgb(0xf0, 0xf4, 0xf9),
    container_low: Color32::from_rgb(0xf8, 0xfa, 0xfd),
    container_high: Color32::from_rgb(0xe9, 0xee, 0xf6),
    on_surface: Color32::from_rgb(0x1f, 0x1f, 0x1f),
    on_variant: Color32::from_rgb(0x44, 0x47, 0x46),
    outline: Color32::from_rgb(0xc4, 0xc7, 0xc5),
    primary: Color32::from_rgb(0x0b, 0x57, 0xd0),
    primary_container: Color32::from_rgb(0xd3, 0xe3, 0xfd),
    selected: Color32::from_rgb(0xc2, 0xe7, 0xff),
    on_selected: Color32::from_rgb(0x00, 0x1d, 0x35),
    on_primary: Color32::WHITE,
    green: Color32::from_rgb(0x14, 0x6c, 0x2e),
    red: Color32::from_rgb(0xb3, 0x26, 0x1e),
};

pub const DARK: Palette = Palette {
    background: Color32::from_rgb(0x13, 0x13, 0x14),
    container: Color32::from_rgb(0x1e, 0x1f, 0x20),
    container_low: Color32::from_rgb(0x1b, 0x1b, 0x1b),
    container_high: Color32::from_rgb(0x28, 0x2a, 0x2c),
    on_surface: Color32::from_rgb(0xe3, 0xe3, 0xe3),
    on_variant: Color32::from_rgb(0xc4, 0xc7, 0xc5),
    outline: Color32::from_rgb(0x44, 0x47, 0x46),
    primary: Color32::from_rgb(0xa8, 0xc7, 0xfa),
    primary_container: Color32::from_rgb(0x08, 0x42, 0xa0),
    selected: Color32::from_rgb(0x00, 0x4a, 0x77),
    on_selected: Color32::from_rgb(0xc2, 0xe7, 0xff),
    on_primary: Color32::from_rgb(0x06, 0x2e, 0x6f),
    green: Color32::from_rgb(0x6d, 0xd5, 0x8c),
    red: Color32::from_rgb(0xf2, 0xb8, 0xb5),
};

pub fn setup_fonts(ctx: &egui::Context) {
    // egui's own font first (keeps its icon glyphs); a system font fills in any letters it lacks.
    let candidates = [
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/opentype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
    ];
    let mut fonts = egui::FontDefinitions::default();
    if let Some(bytes) = candidates.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts.font_data.insert("system".into(), Arc::new(egui::FontData::from_owned(bytes)));
        let fam = fonts.families.entry(egui::FontFamily::Proportional).or_default();
        // Noto Sans reads closest to the original's Google Sans; put it first for text.
        fam.insert(0, "system".into());
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context, p: &Palette, dark: bool) {
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = p.background;
    v.window_fill = p.background;
    v.extreme_bg_color = p.background;
    v.faint_bg_color = p.container_low;
    v.override_text_color = Some(p.on_surface);
    v.selection.bg_fill = p.primary;
    v.selection.stroke = Stroke::new(1.0, p.on_primary);
    v.hyperlink_color = p.primary;
    v.window_stroke = Stroke::new(1.0, p.outline);
    v.window_corner_radius = 16.0.into();
    v.menu_corner_radius = 8.0.into();
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = 20.0.into();
    }
    v.widgets.inactive.weak_bg_fill = p.background;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, p.outline);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.primary);
    v.widgets.hovered.weak_bg_fill = p.container_high;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.outline);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.outline);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.on_variant);
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = vec2(8.0, 6.0);
        s.spacing.button_padding = vec2(16.0, 7.0);
        s.spacing.interact_size.y = 32.0;
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(22.0));
        s.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
    });
}

/// Our own mark: a cloud with a sync arrow (not the original's logo).
/// The app's logo (assets/logo.svg): a cloud carrying a terminal prompt, drawn for this app.
pub const LOGO_SVG: &[u8] = include_bytes!("../assets/logo.svg");

/// The logo at a given size, for the title area and the About dialog.
pub fn logo(size: f32) -> egui::Image<'static> {
    egui::Image::from_bytes("bytes://drive-for-linux-logo.svg", LOGO_SVG).fit_to_exact_size(egui::vec2(size, size))
}

/// The logo as pixels (window, taskbar and tray icon).
pub fn logo_rgba(px: u32) -> Option<Vec<u8>> {
    let tree = resvg::usvg::Tree::from_data(LOGO_SVG, &resvg::usvg::Options::default()).ok()?;
    let mut map = resvg::tiny_skia::Pixmap::new(px, px)?;
    let scale = px as f32 / tree.size().width();
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut map.as_mut());
    // tiny-skia keeps premultiplied colours; icons want straight ones.
    let mut out = map.take();
    for p in out.as_chunks_mut::<4>().0 {
        let a = p[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Some(out)
}

pub fn app_icon() -> egui::IconData {
    let n = 128u32;
    egui::IconData { rgba: logo_rgba(n).unwrap_or_else(|| vec![0; (n * n * 4) as usize]), width: n, height: n }
}

pub fn human_bytes(b: u64) -> String {
    let units = ["bytes", "KB", "MB", "GB", "TB", "PB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} bytes")
    } else if v >= 100.0 {
        format!("{:.0} {}", v, units[i])
    } else {
        format!("{:.1} {}", v, units[i])
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn logo_becomes_an_icon() {
        let px = super::logo_rgba(64).expect("the SVG renders");
        assert_eq!(px.len(), 64 * 64 * 4);
        // corners are transparent (rounded tile), the middle is opaque
        assert_eq!(px[3], 0);
        let mid = (32 * 64 + 32) * 4;
        assert_eq!(px[mid + 3], 255);
        assert_eq!(super::app_icon().width, 128);
    }
}

//! Resolve persisted values into drawing metrics once per widget view.
use crate::theme::{self, Colors};
use iced::{Color, Font};
use std::sync::LazyLock;
use timeschedule_desktop::preferences::{AppTheme, Preferences};

pub fn fonts() -> &'static [&'static str] {
    static FONTS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
        let mut system = iced::advanced::graphics::text::font_system()
            .write()
            .unwrap();
        let names: std::collections::BTreeSet<String> = system
            .raw()
            .db()
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
            .filter(|n| n != "Iced-Icons")
            .collect();
        // Iced Font names require static lifetimes. Intern the bounded installed
        // inventory once, never leak a string per selection or arbitrary input.
        names
            .into_iter()
            .map(|name| &*Box::leak(name.into_boxed_str()))
            .collect()
    });
    &FONTS
}
pub fn font(name: &str) -> Font {
    fonts()
        .iter()
        .find(|n| n.eq_ignore_ascii_case(name))
        .map_or(Font::DEFAULT, |n| Font::with_name(n))
}
pub fn rgb(value: &str) -> Option<Color> {
    if !timeschedule_desktop::widget_preferences::valid_rgb(value) {
        return None;
    }
    let n = u32::from_str_radix(&value[1..], 16).ok()?;
    Some(Color::from_rgb8((n >> 16) as u8, (n >> 8) as u8, n as u8))
}
pub fn hex(color: Color) -> String {
    format!(
        "#{:02X}{:02X}{:02X}",
        (color.r * 255.).round() as u8,
        (color.g * 255.).round() as u8,
        (color.b * 255.).round() as u8
    )
}
#[derive(Clone, Copy)]
pub struct Appearance {
    pub colors: Colors,
    event_colors: Colors,
    pub font: Font,
    pub date_size: f32,
    pub event_size: f32,
    pub date_height: f32,
    pub event_height: f32,
    pub gap: f32,
    pub padding: f32,
}
impl Appearance {
    pub fn event_color(&self, key: &str) -> Color {
        theme::event_color(self.event_colors, key)
    }
    pub fn resolve(prefs: &Preferences) -> Self {
        let w = &prefs.widget;
        let mut p = theme::palette(prefs.theme);
        let overrides = if prefs.theme == AppTheme::Midnight {
            &w.dark
        } else {
            &w.light
        };
        if let Some(c) = overrides.background.as_deref().and_then(rgb) {
            p.background = c;
            p.sidebar = c;
            p.selected = c;
        }
        if let Some(c) = overrides.text.as_deref().and_then(rgb) {
            p.text = c;
            p.muted = c;
        }
        if let Some(c) = overrides.today.as_deref().and_then(rgb) {
            p.accent = c;
            p.on_accent = if c.r * 0.299 + c.g * 0.587 + c.b * 0.114 > 0.6 {
                Color::BLACK
            } else {
                Color::WHITE
            };
        }
        if let Some(c) = overrides.sunday.as_deref().and_then(rgb) {
            p.rose = c;
        }
        if let Some(c) = overrides.saturday.as_deref().and_then(rgb) {
            p.blue = c;
        }
        if let Some(c) = overrides.border.as_deref().and_then(rgb) {
            p.line = c;
        }
        Self {
            colors: p,
            event_colors: theme::palette(prefs.theme),
            font: font(&w.font),
            date_size: w.date_size.into(),
            event_size: w.event_size.into(),
            date_height: f32::from(w.date_size) * 1.3 + 1.,
            event_height: (f32::from(w.event_size) * 1.3 + f32::from(w.line_gap)).max(12.),
            gap: w.cell_gap.into(),
            padding: w.cell_padding.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn weekend_overrides_do_not_recolor_events() {
        let mut prefs = Preferences::default();
        let before = Appearance::resolve(&prefs);
        prefs.widget.dark.saturday = Some("#00FF00".into());
        prefs.widget.dark.sunday = Some("#FFFF00".into());
        let after = Appearance::resolve(&prefs);
        assert_ne!(after.colors.blue, before.colors.blue);
        assert_eq!(after.event_color("blue"), before.event_color("blue"));
        assert_eq!(after.event_color("rose"), before.event_color("rose"));
    }
}

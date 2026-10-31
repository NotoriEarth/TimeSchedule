//! Shared native colors. No image assets or periodic rendering work.
use iced::{
    color,
    widget::{button, container, text},
    Border, Color, Theme,
};
use std::sync::LazyLock;
use timeschedule_desktop::preferences::AppTheme;

#[derive(Clone, Copy)]
pub struct Colors {
    pub background: Color,
    pub sidebar: Color,
    pub surface: Color,
    pub raised: Color,
    pub text: Color,
    pub muted: Color,
    pub line: Color,
    pub accent: Color,
    pub selected: Color,
    pub on_accent: Color,
    pub green: Color,
    pub blue: Color,
    pub rose: Color,
    pub amber: Color,
    pub danger: Color,
}

pub const MIDNIGHT: Colors = Colors {
    background: color!(0x1B1D22),
    sidebar: color!(0x16181D),
    surface: color!(0x20232A),
    raised: color!(0x282C35),
    text: color!(0xE9E9EE),
    muted: color!(0xA1A5B2),
    line: color!(0x343945),
    accent: color!(0xB5A4E9),
    selected: color!(0x343044),
    on_accent: color!(0x211B30),
    green: color!(0x96D9BE),
    blue: color!(0xA4BCFF),
    rose: color!(0xEEA6C3),
    amber: color!(0xE5C48F),
    danger: color!(0xF29AAB),
};
pub const PORCELAIN: Colors = Colors {
    background: color!(0xFFFFFF),
    sidebar: color!(0xF4F6FA),
    surface: color!(0xFAFBFD),
    raised: color!(0xF0F3F8),
    text: color!(0x202632),
    muted: color!(0x626F81),
    line: color!(0xDFE4ED),
    accent: color!(0x4263EB),
    selected: color!(0xE9EDFF),
    on_accent: Color::WHITE,
    green: color!(0x276C52),
    blue: color!(0x315DCC),
    rose: color!(0xA9416F),
    amber: color!(0x875A20),
    danger: color!(0xB42C4B),
};

pub fn palette(mode: AppTheme) -> Colors {
    match mode {
        AppTheme::Midnight => MIDNIGHT,
        AppTheme::Porcelain => PORCELAIN,
    }
}
pub fn colors(theme: &Theme) -> Colors {
    if theme.extended_palette().is_dark {
        MIDNIGHT
    } else {
        PORCELAIN
    }
}
fn make_theme(mode: AppTheme) -> Theme {
    let p = palette(mode);
    Theme::custom(
        match mode {
            AppTheme::Midnight => "Midnight",
            AppTheme::Porcelain => "Porcelain",
        },
        iced::theme::Palette {
            background: p.background,
            text: p.text,
            primary: p.accent,
            success: p.green,
            warning: p.amber,
            danger: p.danger,
        },
    )
}
pub fn iced_theme(mode: AppTheme) -> Theme {
    static DARK: LazyLock<Theme> = LazyLock::new(|| make_theme(AppTheme::Midnight));
    static LIGHT: LazyLock<Theme> = LazyLock::new(|| make_theme(AppTheme::Porcelain));
    match mode {
        AppTheme::Midnight => DARK.clone(),
        AppTheme::Porcelain => LIGHT.clone(),
    }
}
pub fn muted(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(colors(theme).muted),
    }
}
pub fn accent(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(colors(theme).accent),
    }
}
pub fn danger(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(colors(theme).danger),
    }
}
pub fn surface(theme: &Theme) -> container::Style {
    let p = colors(theme);
    container::Style::default()
        .background(p.surface)
        .color(p.text)
        .border(Border {
            color: p.line,
            width: 1.0,
            radius: 8.0.into(),
        })
}
pub fn root(theme: &Theme) -> container::Style {
    let p = colors(theme);
    container::Style::default()
        .background(p.background)
        .color(p.text)
}
pub fn sidebar(theme: &Theme) -> container::Style {
    let p = colors(theme);
    container::Style::default()
        .background(p.sidebar)
        .color(p.text)
}
pub fn inspector(theme: &Theme) -> container::Style {
    let p = colors(theme);
    container::Style::default()
        .background(p.surface)
        .color(p.text)
}
pub fn alert(theme: &Theme) -> container::Style {
    let p = colors(theme);
    container::Style::default()
        .background(p.danger.scale_alpha(0.12))
        .color(p.danger)
        .border(Border {
            radius: 6.0.into(),
            ..Default::default()
        })
}
pub fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let p = colors(theme);
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(if hovered { p.raised } else { p.surface }.into()),
        text_color: if status == button::Status::Disabled {
            p.muted
        } else {
            p.text
        },
        border: Border {
            color: p.line,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}
pub fn text_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = colors(theme);
    button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then_some(p.raised.into()),
        text_color: if status == button::Status::Disabled {
            p.muted
        } else {
            p.text
        },
        border: Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let p = colors(theme);
    let disabled = status == button::Status::Disabled;
    button::Style {
        background: Some(if disabled { p.raised } else { p.accent }.into()),
        text_color: if disabled { p.muted } else { p.on_accent },
        border: Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
pub fn navigation(theme: &Theme, status: button::Status) -> button::Style {
    let p = colors(theme);
    button::Style {
        background: Some(p.selected.into()),
        text_color: p.accent,
        ..text_button(theme, status)
    }
}
pub fn event_color(p: Colors, category: &str) -> Color {
    match category {
        "blue" => p.blue,
        "amber" => p.amber,
        "rose" => p.rose,
        _ => p.green,
    }
}
pub fn event_surface(theme: &Theme, category: &str) -> container::Style {
    let p = colors(theme);
    container::Style::default()
        .background(event_color(p, category).scale_alpha(0.12))
        .color(p.text)
        .border(Border {
            radius: 5.0.into(),
            ..Default::default()
        })
}

use iced::widget::text;

// Measure with the same font/shaping as the rendered chip. Character counts
// alone do not bound mixed Korean/Latin titles, especially at minimum width.
#[cfg(test)]
fn text_width(content: &str, size: f32) -> f32 {
    text_width_with_font(content, size, iced::Font::with_name("Malgun Gothic"))
}
pub(crate) fn text_width_with_font(content: &str, size: f32, font: iced::Font) -> f32 {
    use iced::advanced::{
        graphics::text::Paragraph,
        text::{Paragraph as _, Text},
    };
    Paragraph::with_text(Text {
        content,
        bounds: iced::Size::new(f32::INFINITY, size * 1.3),
        size: size.into(),
        line_height: Default::default(),
        font,
        align_x: Default::default(),
        align_y: iced::alignment::Vertical::Top,
        shaping: Default::default(),
        wrapping: text::Wrapping::None,
    })
    .min_width()
}

pub fn fit_label(content: &str, width: f32, size: f32) -> String {
    fit_label_with_font(content, width, size, iced::Font::with_name("Malgun Gothic"))
}
pub fn fit_label_with_font(content: &str, width: f32, size: f32, font: iced::Font) -> String {
    let content = content.replace(['\n', '\r', '\t'], " ");
    if text_width_with_font(&content, size, font) <= width {
        return content;
    }
    if text_width_with_font("…", size, font) > width {
        return String::new();
    }
    let boundaries: Vec<_> = content.char_indices().map(|(index, _)| index).collect();
    let (mut low, mut high) = (0, boundaries.len());
    while low < high {
        let mid = (low + high + 1) / 2;
        let end = boundaries.get(mid).copied().unwrap_or(content.len());
        if text_width_with_font(&format!("{}…", &content[..end]), size, font) <= width {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    format!(
        "{}…",
        &content[..boundaries.get(low).copied().unwrap_or(content.len())]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chosen_font_bounds_match_the_rendered_title() {
        for name in ["Malgun Gothic", "Arial", "Consolas"] {
            let font = crate::widget_appearance::font(name);
            for size in [9.0, 18.0, 24.0] {
                for width in [0.0, 25.0, 71.0, 190.0] {
                    let fitted =
                        fit_label_with_font("WWWW 한글 일정 테스트 123456", width, size, font);
                    assert!(text_width_with_font(&fitted, size, font) <= width + 0.1);
                }
            }
        }
    }
    #[test]
    fn mixed_language_calendar_titles_fit_their_actual_width() {
        for title in [
            "아주 긴 일정 제목과 English WWWWW",
            "✓ 좁은 칸의 완료 일정",
            "한 줄\n다음 줄",
            "🙂 한글 일정",
        ] {
            for width in [0.0, 8.0, 28.0, 53.0, 95.0, 180.0] {
                for size in [10.0, 11.0, 15.0] {
                    let fitted = fit_label(title, width, size);
                    assert!(
                        text_width(&fitted, size) <= width + 0.1,
                        "{fitted}: {width}"
                    );
                    assert!(!fitted.contains('\n'));
                }
            }
        }
    }
}

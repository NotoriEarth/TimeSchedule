//! Optional snapshots from the real native renderer, without a desktop session.
use super::*;
use iced::advanced::{graphics::Viewport, layout, mouse, renderer, widget::Tree, Renderer};
use iced::{Point, Rectangle};

#[test]
#[ignore = "manual native-renderer preview; writes PNG files to TIMESCHEDULE_PREVIEW_DIR"]
fn render_theme_previews() {
    let output = PathBuf::from(
        std::env::var_os("TIMESCHEDULE_PREVIEW_DIR").expect("set an output directory"),
    );
    std::fs::create_dir_all(&output).unwrap();
    let mut app = fixture();
    app.selected = NaiveDate::from_ymd_opt(2026, 10, 31).unwrap();
    let store = app.store.as_ref().unwrap();
    for (day, title, start, color) in [
        (7, "독서 모임", "19:00", "sage"),
        (12, "개발 기록 정리", "10:00", "rose"),
        (20, "프로젝트 리뷰", "14:00", "blue"),
        (24, "주간 회고", "17:00", "amber"),
        (31, "기능 설계", "09:00", "sage"),
        (31, "프로젝트 리뷰", "14:00", "rose"),
        (
            31,
            "아주 긴 제목의 일정이 옆 날짜로 넘어가지 않도록 확인",
            "17:00",
            "blue",
        ),
    ] {
        let mut f = Form::new(Kind::Event, format!("2026-10-{day:02}"), None);
        f.title = title.into();
        f.start = start.into();
        f.end = format!("{:02}:00", start[..2].parse::<u32>().unwrap() + 1);
        f.color = color.into();
        f.all_day = false;
        f.save(store).unwrap();
    }
    for (kind, title) in [
        (Kind::Project, "TimeSchedule"),
        (Kind::Project, "개인 공부"),
        (Kind::Task, "달력 UI 다듬기"),
        (Kind::Task, "개발 기록 정리"),
        (Kind::Entry, "작은 개선의 기록"),
    ] {
        let mut f = Form::new(kind, app.selected.to_string(), None);
        f.title = title.into();
        f.body = "작은 개선을 차곡차곡 기록하기.".into();
        f.save(store).unwrap();
    }
    app.snapshot = worker::snapshot(store, &app.scope()).unwrap();
    for (name, mode, size) in [
        ("midnight", AppTheme::Midnight, Size::new(1440.0, 940.0)),
        ("porcelain", AppTheme::Porcelain, Size::new(1440.0, 940.0)),
        (
            "midnight-default",
            AppTheme::Midnight,
            Size::new(1160.0, 820.0),
        ),
        (
            "midnight-short",
            AppTheme::Midnight,
            Size::new(1160.0, 620.0),
        ),
        (
            "porcelain-narrow",
            AppTheme::Porcelain,
            Size::new(800.0, 620.0),
        ),
    ] {
        app.preferences.theme = mode;
        app.manager_size = size;
        let mut renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
        let bounds = Rectangle::new(Point::ORIGIN, size);
        renderer.reset(bounds);
        let mut element = app.view(app.manager);
        let mut tree = Tree::new(element.as_widget());
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        let theme = app.theme(app.manager);
        let colors = crate::theme::palette(mode);
        // Iced initializes control interaction styles during a redraw event.
        // Drawing a freshly laid-out tree without it would show disabled styles.
        let mut messages = Vec::new();
        element.as_widget_mut().update(
            &mut tree,
            &iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now())),
            layout::Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut iced::advanced::Shell::new(&mut messages),
            &bounds,
        );
        element.as_widget().draw(
            &tree,
            &mut renderer,
            &theme,
            &renderer::Style {
                text_color: colors.text,
            },
            layout::Layout::new(&node),
            mouse::Cursor::Unavailable,
            &bounds,
        );
        let width = size.width as u32;
        let height = size.height as u32;
        let mut pixmap = tiny_skia::Pixmap::new(width, height).unwrap();
        let mut mask = tiny_skia::Mask::new(width, height).unwrap();
        renderer.draw(
            &mut pixmap.as_mut(),
            &mut mask,
            &Viewport::with_physical_size(Size::new(width, height), 1.0),
            &[bounds],
            colors.background,
        );
        let file = std::fs::File::create(output.join(format!("{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        // The software window renderer targets softbuffer's BGR byte order.
        // The root is opaque, so only red/blue need swapping for an RGBA PNG.
        let mut rgba = pixmap.data().to_vec();
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&rgba)
            .unwrap();
    }
}

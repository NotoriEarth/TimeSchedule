//! Optional snapshots from the real native renderer, without a desktop session.
use super::*;
use iced::advanced::{graphics::Viewport, layout, mouse, renderer, widget::Tree, Renderer};
use iced::{Point, Rectangle};

#[test]
fn month_heading_has_separate_year_and_month_targets() {
    use crate::month_jump::{JumpField, JumpMessage};
    let mut app = fixture();
    app.widget = Some(window::Id::unique());
    app.widget_attached = true;
    let size = Size::new(960., 640.);
    let bounds = Rectangle::new(Point::ORIGIN, size);
    let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
    let mut element = app.view(app.widget.unwrap());
    let mut tree = Tree::new(element.as_widget());
    let node = element.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, size),
    );
    let mut targets = Vec::new();
    fn collect(node: &layout::Node, parent: Point, points: &mut Vec<Point>) {
        let b = node.bounds();
        let origin = Point::new(parent.x + b.x, parent.y + b.y);
        if origin.y < 100. && b.width > 8. && b.width < 200. && b.height > 8. && b.height < 60. {
            points.push(Point::new(
                origin.x + b.width / 2.,
                origin.y + b.height / 2.,
            ));
        }
        for child in node.children() {
            collect(child, origin, points);
        }
    }
    collect(&node, Point::ORIGIN, &mut targets);
    let mut messages = Vec::new();
    for point in targets {
        for event in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            element.as_widget_mut().update(
                &mut tree,
                &iced::Event::Mouse(event),
                layout::Layout::new(&node),
                mouse::Cursor::Available(point),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut iced::advanced::Shell::new(&mut messages),
                &bounds,
            );
        }
    }
    assert!(messages
        .iter()
        .any(|m| matches!(m, Message::Jump(JumpMessage::Open(JumpField::Year)))));
    assert!(messages
        .iter()
        .any(|m| matches!(m, Message::Jump(JumpMessage::Open(JumpField::Month)))));
    assert!(!messages.iter().any(|m| matches!(m, Message::DragWidget)));
}

#[test]
fn widget_checkbox_clicks_do_not_open_a_day_but_double_click_does() {
    for busy in [false, true] {
        let mut app = fixture();
        app.busy = busy;
        let date = calendar_days(app.widget_month)[0];
        let mut form = Form::new(Kind::Event, date.to_string(), None);
        form.title = "완료할 일정".into();
        form.save(app.store.as_ref().unwrap()).unwrap();
        app.snapshot = worker::snapshot(app.store.as_ref().unwrap(), &app.scope()).unwrap();
        app.widget = Some(window::Id::unique());
        app.widget_attached = true;
        let size = Size::new(960.0, 640.0);
        let bounds = Rectangle::new(Point::ORIGIN, size);
        let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
        let mut element = app.view(app.widget.unwrap());
        let mut tree = Tree::new(element.as_widget());
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        fn checkboxes(node: &layout::Node, parent: Point, found: &mut Vec<Point>) {
            let b = node.bounds();
            let origin = Point::new(parent.x + b.x, parent.y + b.y);
            if (b.width - 12.0).abs() < 0.1 && (b.height - 12.0).abs() < 0.1 {
                found.push(Point::new(origin.x + 6.0, origin.y + 6.0));
            }
            for child in node.children() {
                checkboxes(child, origin, found);
            }
        }
        let mut points = Vec::new();
        checkboxes(&node, Point::ORIGIN, &mut points);
        assert!(
            !points.is_empty(),
            "a real completion control must be present"
        );
        let mut messages = Vec::new();
        let mut dispatch = |event, point, messages: &mut Vec<Message>| {
            element.as_widget_mut().update(
                &mut tree,
                &event,
                layout::Layout::new(&node),
                mouse::Cursor::Available(point),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut iced::advanced::Shell::new(messages),
                &bounds,
            );
        };
        let click = |press| {
            iced::Event::Mouse(if press {
                mouse::Event::ButtonPressed(mouse::Button::Left)
            } else {
                mouse::Event::ButtonReleased(mouse::Button::Left)
            })
        };
        for _ in 0..2 {
            dispatch(click(true), points[0], &mut messages);
            dispatch(click(false), points[0], &mut messages);
        }
        assert!(
            busy || messages
                .iter()
                .any(|m| matches!(m, Message::CompleteWidgetEvent(_, true)))
        );
        assert!(!messages.iter().any(|m| matches!(m, Message::WidgetDate(_))));
        messages.clear();
        let empty = Point::new(points[0].x + 60.0, points[0].y + 20.0);
        dispatch(click(true), empty, &mut messages);
        dispatch(click(false), empty, &mut messages);
        assert!(
            messages.is_empty(),
            "single date clicks must not open a window"
        );
        dispatch(click(true), empty, &mut messages);
        dispatch(click(false), empty, &mut messages);
        assert!(matches!(&messages[..], [Message::WidgetDate(d)] if *d == date));
    }
}

#[test]
fn widget_background_alpha_changes_without_fading_solid_ink() {
    for mode in [AppTheme::Midnight, AppTheme::Porcelain] {
        let mut app = fixture();
        app.preferences.theme = mode;
        let id = window::Id::unique();
        app.widget = Some(id);
        app.widget_attached = true;
        let render = |app: &App| {
            let size = Size::new(960.0, 640.0);
            let bounds = Rectangle::new(Point::ORIGIN, size);
            let mut renderer =
                iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
            renderer.reset(bounds);
            let mut element = app.view(id);
            let mut tree = Tree::new(element.as_widget());
            let node = element.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, size),
            );
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
                &app.theme(id),
                &renderer::Style {
                    text_color: crate::theme::palette(mode).text,
                },
                layout::Layout::new(&node),
                mouse::Cursor::Unavailable,
                &bounds,
            );
            let mut pixels = tiny_skia::Pixmap::new(960, 640).unwrap();
            renderer.draw(
                &mut pixels.as_mut(),
                &mut tiny_skia::Mask::new(960, 640).unwrap(),
                &Viewport::with_physical_size(Size::new(960, 640), 1.0),
                &[bounds],
                iced::Color::TRANSPARENT,
            );
            pixels.data().to_vec()
        };
        app.preferences.widget_opacity = 100;
        let opaque = render(&app);
        app.preferences.widget_opacity = 40;
        let translucent = render(&app);
        let partial = translucent.chunks_exact(4).filter(|p| p[3] == 102).count();
        assert!(
            partial > 100_000,
            "large cell interiors must use 40% alpha, got {partial}"
        );
        let ink = crate::theme::palette(mode).text;
        let ink = [
            (ink.b * 255.0).round() as u8,
            (ink.g * 255.0).round() as u8,
            (ink.r * 255.0).round() as u8,
            255,
        ];
        let retained = opaque
            .chunks_exact(4)
            .zip(translucent.chunks_exact(4))
            .filter(|(a, b)| *a == ink && *b == ink)
            .count();
        assert!(
            retained > 100,
            "opaque text pixels must retain their exact color: {retained}"
        );
        assert!(
            translucent.chunks_exact(4).any(|p| p[3] == 0),
            "gaps must be transparent"
        );
    }
}

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
    let desktop = window::Id::unique();
    app.widget = Some(desktop);
    app.widget_attached = true;
    let quick = window::Id::unique();
    let jump_id = window::Id::unique();
    app.quick_window = Some(quick);
    app.quick_date = Some(app.selected);
    app.quick_events = store
        .list_events(&app.selected.to_string(), &app.selected.to_string())
        .unwrap();
    for (name, mode, size) in [
        ("jump-midnight", AppTheme::Midnight, Size::new(400.0, 300.0)),
        (
            "jump-porcelain",
            AppTheme::Porcelain,
            Size::new(400.0, 300.0),
        ),
        (
            "settings-midnight",
            AppTheme::Midnight,
            Size::new(1160.0, 1100.0),
        ),
        (
            "settings-porcelain",
            AppTheme::Porcelain,
            Size::new(1160.0, 1100.0),
        ),
        (
            "desktopcal-rolling",
            AppTheme::Midnight,
            Size::new(800.0, 600.0),
        ),
        (
            "desktopcal-custom-small",
            AppTheme::Porcelain,
            Size::new(400.0, 350.0),
        ),
        ("quick-list", AppTheme::Midnight, Size::new(440.0, 480.0)),
        ("quick-editor", AppTheme::Midnight, Size::new(440.0, 480.0)),
        (
            "quick-editor-small",
            AppTheme::Porcelain,
            Size::new(350.0, 330.0),
        ),
        (
            "quick-confirm-small",
            AppTheme::Porcelain,
            Size::new(350.0, 330.0),
        ),
        (
            "desktopcal-midnight",
            AppTheme::Midnight,
            Size::new(960.0, 640.0),
        ),
        (
            "desktopcal-porcelain",
            AppTheme::Porcelain,
            Size::new(960.0, 640.0),
        ),
        (
            "desktopcal-fitted",
            AppTheme::Midnight,
            Size::new(500.0, 440.0),
        ),
        (
            "desktopcal-small",
            AppTheme::Porcelain,
            Size::new(320.0, 300.0),
        ),
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
        app.preferences.widget = Default::default();
        app.preferences.widget_locked = !name.starts_with("desktopcal-");
        app.page = if name.starts_with("settings-") {
            Page::Settings
        } else {
            Page::Month
        };
        app.widget_ui.jump = if name.starts_with("jump-") {
            Some(crate::month_jump::MonthJump {
                window: jump_id,
                focus: crate::month_jump::JumpField::Month,
                year: "2025".into(),
                month: "9".into(),
                date: String::new(),
                error: String::new(),
            })
        } else {
            None
        };
        if name == "desktopcal-rolling" {
            app.preferences.widget.rolling = true;
            app.preferences.widget.weeks = 4;
            app.preferences.widget.past_weeks = 1;
            app.preferences.widget.event_size = 14;
            app.preferences.widget.cell_gap = 3;
            app.preferences.widget.dark.background = Some("#152D32".into());
            app.preferences.widget.dark.today = Some("#74D3B7".into());
        }
        if name == "desktopcal-custom-small" {
            app.preferences.widget.rolling = true;
            app.preferences.widget.weeks = 2;
            app.preferences.widget.font = "Arial".into();
            app.preferences.widget.date_size = 32;
            app.preferences.widget.event_size = 24;
            app.preferences.widget.cell_gap = 16;
            app.preferences.widget.cell_padding = 20;
        }
        app.manager_size = size;
        app.editor = None;
        app.editor_in_quick = false;
        if name.starts_with("quick-") && name != "quick-list" {
            let mut editor = Editor::new(Form::event(app.quick_events[0].clone()), 1);
            editor.form.title.push_str(" 수정 중");
            if name == "quick-confirm-small" {
                editor.request_close(CloseIntent::Quick);
            }
            app.editor = Some(editor);
            app.editor_in_quick = true;
        }
        app.preferences.widget_opacity = 65;
        let mut renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
        let bounds = Rectangle::new(Point::ORIGIN, size);
        renderer.reset(bounds);
        let mut element = app.view(if name.starts_with("jump-") {
            jump_id
        } else if name.starts_with("desktopcal-") {
            desktop
        } else if name.starts_with("quick-") {
            quick
        } else {
            app.manager
        });
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
            iced::Color::TRANSPARENT,
        );
        let file = std::fs::File::create(output.join(format!("{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        // The software window renderer targets softbuffer's BGR byte order.
        // PNG expects straight RGBA; softbuffer uses premultiplied BGRA.
        let mut rgba = pixmap.data().to_vec();
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            let alpha = pixel[3] as u32;
            if alpha != 0 {
                for channel in &mut pixel[..3] {
                    *channel = ((*channel as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
                }
            }
        }
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&rgba)
            .unwrap();
    }
}

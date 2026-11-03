//! Optional snapshots from the real native renderer, without a desktop session.
use super::*;
use iced::advanced::{graphics::Viewport, layout, mouse, renderer, widget::Tree, Renderer};
use iced::{Point, Rectangle};

#[derive(Default)]
struct SettingsProbe {
    set_scroll: Option<f32>,
    set_scroll_index: usize,
    texts: Vec<String>,
    scrolls: Vec<(Rectangle, Rectangle, iced::Vector)>,
    controls: Vec<(String, Rectangle)>,
}
impl iced::advanced::widget::Operation for SettingsProbe {
    fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation)) {
        visit(self);
    }
    fn scrollable(
        &mut self,
        _: Option<&iced::advanced::widget::Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: iced::Vector,
        state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        if self.scrolls.len() == self.set_scroll_index {
            if let Some(y) = self.set_scroll {
                state.scroll_to(iced::widget::scrollable::AbsoluteOffset {
                    x: None,
                    y: Some(y),
                });
            }
        }
        self.scrolls.push((bounds, content, translation));
    }
    fn text(&mut self, _: Option<&iced::advanced::widget::Id>, bounds: Rectangle, value: &str) {
        self.texts.push(value.to_owned());
        if [
            "위젯 숨기기",
            "위젯 표시",
            "위젯 위치 복구",
            "잠금 해제",
            "크기와 표시 기간",
            "크기 적용",
            "글꼴과 간격",
        ]
        .contains(&value)
            || value.starts_with("배경 불투명도")
        {
            self.controls.push((value.to_owned(), bounds));
        }
    }
}

fn render_settings_frame(
    app: &App,
    element: &mut Element<'_, Message>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &mut iced::Renderer,
    size: Size,
    output: &std::path::Path,
) {
    let bounds = Rectangle::new(Point::ORIGIN, size);
    renderer.reset(bounds);
    element.as_widget_mut().update(
        tree,
        &iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now())),
        layout::Layout::new(&node),
        mouse::Cursor::Unavailable,
        renderer,
        &mut iced::advanced::clipboard::Null,
        &mut iced::advanced::Shell::new(&mut Vec::new()),
        &bounds,
    );
    element.as_widget().draw(
        tree,
        renderer,
        &app.theme(app.manager),
        &renderer::Style {
            text_color: crate::theme::palette(app.preferences.theme).text,
        },
        layout::Layout::new(&node),
        mouse::Cursor::Unavailable,
        &bounds,
    );
    let mut pixels = tiny_skia::Pixmap::new(size.width as u32, size.height as u32).unwrap();
    renderer.draw(
        &mut pixels.as_mut(),
        &mut tiny_skia::Mask::new(size.width as u32, size.height as u32).unwrap(),
        &Viewport::with_physical_size(Size::new(size.width as u32, size.height as u32), 1.),
        &[bounds],
        crate::theme::palette(app.preferences.theme).background,
    );
    let mut rgba = pixels.data().to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let file = std::fs::File::create(output).unwrap();
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        size.width as u32,
        size.height as u32,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&rgba)
        .unwrap();
}

#[test]
fn settings_retained_layout_is_stable_across_native_busy_transitions() {
    // Keep the real scrollable tree alive across each change, as Iced does.
    // Removing the native controls while busy must fail this geometry check.
    for mode in [AppTheme::Midnight, AppTheme::Porcelain] {
        for size in [Size::new(620., 480.), Size::new(1160., 820.)] {
            let mut app = fixture();
            app.preferences.theme = mode;
            let mut renderer =
                iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
            let mut tree = Tree::empty();
            let mut baseline = None;
            let mut scroll_baseline = None;
            let mut status_baseline = None;
            let mut controls_baseline = None;
            for (frame, busy) in [false, true, false, true, false].into_iter().enumerate() {
                app.native_busy = busy;
                app.desktop_message = if frame < 3 {
                    "바탕화면 부모 연결 확인 (WorkerW). 위젯이 작업 영역 안에 표시됩니다.".into()
                } else {
                    "위젯 화면 보정 실패: 바탕화면 호스트가 변경되었습니다.\n화면 연결을 확인한 다음 위젯 위치 복구를 선택하세요.\n연결 상태가 복구될 때까지 사용자 선호 위치와 크기를 보존합니다.\n추가 오류 내용도 이 영역 안에서 스크롤해 확인할 수 있습니다.".into()
                };
                let mut element = crate::views::settings(&app);
                tree.diff(element.as_widget());
                let node = element.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, size),
                );
                fn geometry(node: &layout::Node, parent: Point, out: &mut Vec<(f32, f32, f32)>) {
                    let b = node.bounds();
                    let p = Point::new(parent.x + b.x, parent.y + b.y);
                    // Text widths may change with the status, but every target,
                    // vertical position, height and scroll-content extent must remain.
                    out.push((p.x, p.y, b.height));
                    for child in node.children() {
                        geometry(child, p, out);
                    }
                }
                let mut positions = Vec::new();
                geometry(&node, Point::ORIGIN, &mut positions);
                if let Some(expected) = &baseline {
                    if frame < 3 {
                        assert_eq!(
                            &positions, expected,
                            "settings jumped: {mode:?} {size:?} busy={busy}"
                        );
                    }
                } else {
                    baseline = Some(positions);
                }
                if frame == 0 {
                    let mut set = SettingsProbe {
                        set_scroll: Some(460.),
                        ..Default::default()
                    };
                    element.as_widget_mut().operate(
                        &mut tree,
                        layout::Layout::new(&node),
                        &renderer,
                        &mut set,
                    );
                }
                let mut probe = SettingsProbe::default();
                element.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut probe,
                );
                assert!(
                    probe.scrolls[0].2.y > 0.,
                    "fixture must be scrolled to the widget section"
                );
                assert!(
                    probe.controls.len() >= 6,
                    "real native controls must remain mounted"
                );
                if let Some(expected) = &scroll_baseline {
                    assert_eq!(
                        &probe.scrolls[0], expected,
                        "scroll state changed on frame{frame}"
                    );
                } else {
                    scroll_baseline = Some(probe.scrolls[0]);
                }
                if let Some(expected) = &status_baseline {
                    assert_eq!(
                        &probe.scrolls[1].0, expected,
                        "status viewport must retain its width and position too"
                    );
                } else {
                    status_baseline = Some(probe.scrolls[1].0);
                }
                if let Some(expected) = &controls_baseline {
                    assert_eq!(&probe.controls, expected, "control targets changed");
                } else {
                    controls_baseline = Some(probe.controls);
                }

                assert_eq!(
                    probe.scrolls[1].0.height, 52.,
                    "status viewport keeps its height"
                );
                if frame >= 3 {
                    assert!(
                        probe.scrolls[1].1.height > 52.,
                        "long errors remain scrollable"
                    );
                }
                if size.width == 620. && frame < 4 {
                    if let Some(output) = std::env::var_os("TIMESCHEDULE_SETTINGS_PREVIEW_DIR") {
                        let output = PathBuf::from(output);
                        std::fs::create_dir_all(&output).unwrap();
                        render_settings_frame(
                            &app,
                            &mut element,
                            &mut tree,
                            &node,
                            &mut renderer,
                            size,
                            &output.join(format!("settings-{mode:?}-{frame}.png")),
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn full_settings_shell_keeps_layout_through_real_native_errors_and_recovery() {
    for mode in [AppTheme::Midnight, AppTheme::Porcelain] {
        for size in [Size::new(800., 620.), Size::new(1160., 820.)] {
            for source in ["geometry", "native", "observer"] {
                for unrelated in ["", "다이어리 저장 실패: 별도 오류 fixture"] {
                    let mut app = fixture();
                    app.page = Page::Settings;
                    app.manager_size = size;
                    app.preferences.theme = mode;
                    app.widget = Some(window::Id::unique());
                    app.widget_attached = true;
                    app.desktop_message = "바탕화면 정상 연결 fixture".into();
                    app.error = unrelated.into();
                    let mut renderer =
                        iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
                    let mut tree = Tree::empty();
                    let mut baseline = None;
                    for (frame, failed) in [false, true, false, true, false].into_iter().enumerate()
                    {
                        let failure =
                            format!("{source} 오류 fixture\n줄바꿈이 있는 실제 실패 알림");
                        match source {
                            "geometry" => app.apply_geometry_event(windows::GeometryEvent {
                                preferred: None,
                                actual_size: (960, 640),
                                error: failed.then_some(failure.clone()),
                            }),
                            "native" => {
                                let _ = app.update(Message::NativeDone(
                                    NativeAction::Recover,
                                    if failed {
                                        Err(failure.clone())
                                    } else {
                                        Ok(NativeReport {
                                            attached: true,
                                            message: "바탕화면 정상 연결 fixture".into(),
                                            locked: true,
                                            opacity: None,
                                            visible: None,
                                            position: None,
                                            size: None,
                                        })
                                    },
                                ));
                            }
                            _ => {
                                let _ = app.update(Message::DisplayObserverReady(if failed {
                                    Err(failure.clone())
                                } else {
                                    Ok(())
                                }));
                            }
                        }
                        // Full manager view includes the shell's error banner.
                        // Testing settings() alone cannot see this regression.
                        let mut element = app.view(app.manager);
                        tree.diff(element.as_widget());
                        let node = element.as_widget_mut().layout(
                            &mut tree,
                            &renderer,
                            &layout::Limits::new(Size::ZERO, size),
                        );
                        if frame == 0 {
                            let mut set = SettingsProbe {
                                set_scroll: Some(460.),
                                set_scroll_index: 1,
                                ..Default::default()
                            };
                            element.as_widget_mut().operate(
                                &mut tree,
                                layout::Layout::new(&node),
                                &renderer,
                                &mut set,
                            );
                        }
                        let mut probe = SettingsProbe::default();
                        element.as_widget_mut().operate(
                            &mut tree,
                            layout::Layout::new(&node),
                            &renderer,
                            &mut probe,
                        );
                        assert_eq!(
                            probe.scrolls.len(),
                            3,
                            "sidebar, settings and fixed status must remain mounted"
                        );
                        if size.width == 800.
                            && source == "geometry"
                            && unrelated.is_empty()
                            && frame < 3
                        {
                            if let Some(output) =
                                std::env::var_os("TIMESCHEDULE_FULL_SETTINGS_PREVIEW_DIR")
                            {
                                let output = PathBuf::from(output);
                                std::fs::create_dir_all(&output).unwrap();
                                render_settings_frame(
                                    &app,
                                    &mut element,
                                    &mut tree,
                                    &node,
                                    &mut renderer,
                                    size,
                                    &output.join(format!("full-settings-{mode:?}-{frame}.png")),
                                );
                            }
                        }
                        let actual = (probe.scrolls[1], probe.scrolls[2].0, probe.controls);
                        if let Some(expected) = &baseline {
                            assert_eq!(
                                &actual, expected,
                                "full shell jumped: {mode:?} {size:?} {source} frame{frame}"
                            );
                        } else {
                            baseline = Some(actual);
                        }
                        assert_eq!(
                            app.error, unrelated,
                            "native events must preserve unrelated application errors"
                        );
                        if !unrelated.is_empty() {
                            assert!(
                                probe.texts.iter().any(|text| text.contains(unrelated)),
                                "do not hide unrelated failures"
                            );
                        }
                        assert_eq!(probe.texts.iter().any(|text| text.contains(&failure)), failed,
                            "failure must be visible only until its own recovery: {source} frame{frame}");
                    }
                }
            }
        }
    }
}

#[test]
fn native_error_sources_clear_independently_and_remain_visible_outside_settings() {
    let mut app = fixture();
    app.error = "다이어리 저장 오류 fixture".into();
    let _ = app.update(Message::NativeDone(
        NativeAction::Recover,
        Err("native fixture failure".into()),
    ));
    let _ = app.update(Message::DisplayObserverReady(Err(
        "observer fixture failure".into(),
    )));
    app.apply_geometry_event(windows::GeometryEvent {
        preferred: None,
        actual_size: (960, 640),
        error: Some("geometry fixture failure".into()),
    });
    let visible_text = |app: &App| {
        let size = Size::new(1160., 820.);
        let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
        let mut element = app.view(app.manager);
        let mut tree = Tree::new(element.as_widget());
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        let mut probe = SettingsProbe::default();
        element.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut probe,
        );
        probe.texts.join("\n")
    };
    let all = visible_text(&app);
    for message in [
        "다이어리 저장 오류 fixture",
        "native fixture failure",
        "observer fixture failure",
        "geometry fixture failure",
    ] {
        assert!(all.contains(message));
    }
    app.apply_geometry_event(windows::GeometryEvent {
        preferred: None,
        actual_size: (960, 640),
        error: None,
    });
    let recovered = visible_text(&app);
    assert!(!recovered.contains("geometry fixture failure"));
    for message in [
        "다이어리 저장 오류 fixture",
        "native fixture failure",
        "observer fixture failure",
    ] {
        assert!(recovered.contains(message));
    }
    let _ = app.update(Message::DisplayObserverReady(Ok(())));
    let recovered = visible_text(&app);
    assert!(!recovered.contains("observer fixture failure"));
    assert!(recovered.contains("native fixture failure"));
    assert_eq!(app.error, "다이어리 저장 오류 fixture");
}

#[test]
fn settings_search_keeps_native_errors_visible_until_settings_returns() {
    for mode in [AppTheme::Midnight, AppTheme::Porcelain] {
        for source in ["geometry", "native", "observer"] {
            let mut app = fixture();
            app.page = Page::Settings;
            app.preferences.theme = mode;
            app.error = "별도 저장 오류 fixture".into();
            let size = Size::new(1160., 820.);
            let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
            let mut tree = Tree::empty();
            let mut inspect = |app: &App| {
                let mut element = app.view(app.manager);
                tree.diff(element.as_widget());
                let node = element.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, size),
                );
                let mut probe = SettingsProbe::default();
                element.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut probe,
                );
                probe
            };
            let _ = app.update(Message::Search("실제 검색 전환 fixture".into()));
            assert!(app.searching);
            let error = format!("{source} search failure fixture");
            match source {
                "geometry" => app.apply_geometry_event(windows::GeometryEvent {
                    preferred: None,
                    actual_size: (960, 640),
                    error: Some(error.clone()),
                }),
                "native" => {
                    let _ = app.update(Message::NativeDone(
                        NativeAction::Recover,
                        Err(error.clone()),
                    ));
                }
                _ => {
                    let _ = app.update(Message::DisplayObserverReady(Err(error.clone())));
                }
            }
            for searching in [true, false] {
                if !searching {
                    let _ = app.update(Message::SearchDone(app.search_generation, Ok(Vec::new())));
                }
                let probe = inspect(&app);
                assert!(probe.texts.iter().any(|text| text == "검색 결과"));
                assert!(
                    probe.controls.is_empty(),
                    "the fixed settings status is not rendered during search"
                );
                assert_eq!(probe.texts.iter().filter(|text| text.contains(&error)).count(), 1,
                    "native failure disappeared during Settings search: {mode:?} {source} searching={searching}");
                assert!(probe
                    .texts
                    .iter()
                    .any(|text| text.contains("별도 저장 오류 fixture")));
            }
            for empty in ["", "   "] {
                let _ = app.update(Message::Search(empty.into()));
                let probe = inspect(&app);
                assert!(!probe.texts.iter().any(|text| text == "검색 결과"));
                assert!(
                    !probe.controls.is_empty(),
                    "clearing search restores settings"
                );
                assert_eq!(probe.scrolls.last().unwrap().0.height, 52.);
                assert_eq!(probe.texts.iter().filter(|text| text.contains(&error)).count(), 1,
                    "returning to settings must use its fixed status once, without a duplicate banner");
                assert_eq!(app.error, "별도 저장 오류 fixture");
                assert!(probe.texts.iter().any(|text| text.contains(&app.error)));
            }
        }
    }
}

#[test]
fn diary_and_widget_headings_have_separate_year_and_month_targets() {
    for diary in [false, true] {
        use crate::month_jump::{JumpField, JumpMessage};
        let mut app = fixture();
        app.widget = Some(window::Id::unique());
        app.widget_attached = true;
        let size = Size::new(960., 640.);
        let bounds = Rectangle::new(Point::ORIGIN, size);
        let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
        let mut element = app.view(if diary {
            app.manager
        } else {
            app.widget.unwrap()
        });
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
            if origin.y < 100. && b.width > 8. && b.width < 200. && b.height > 8. && b.height < 60.
            {
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
            .any(|m| matches!(m, Message::Jump(JumpMessage::Open(target, JumpField::Year)) if *target == if diary { crate::month_jump::JumpTarget::Diary } else { crate::month_jump::JumpTarget::Widget })));
        assert!(messages
            .iter()
            .any(|m| matches!(m, Message::Jump(JumpMessage::Open(target, JumpField::Month)) if *target == if diary { crate::month_jump::JumpTarget::Diary } else { crate::month_jump::JumpTarget::Widget })));
        assert!(!messages.iter().any(|m| matches!(m, Message::DragWidget)));
    }
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
        (
            "jump-midnight",
            AppTheme::Midnight,
            crate::month_jump::PICKER_SIZE,
        ),
        (
            "jump-year-midnight",
            AppTheme::Midnight,
            crate::month_jump::PICKER_SIZE,
        ),
        (
            "jump-year-porcelain",
            AppTheme::Porcelain,
            crate::month_jump::PICKER_SIZE,
        ),
        (
            "jump-year-boundary",
            AppTheme::Midnight,
            crate::month_jump::PICKER_MIN_SIZE,
        ),
        (
            "jump-direct-small",
            AppTheme::Porcelain,
            crate::month_jump::PICKER_MIN_SIZE,
        ),
        (
            "jump-porcelain",
            AppTheme::Porcelain,
            crate::month_jump::PICKER_SIZE,
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
        ("quick-editor", AppTheme::Midnight, Size::new(650.0, 560.0)),
        ("diary-editor", AppTheme::Midnight, Size::new(1160.0, 820.0)),
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
        if std::env::var("TIMESCHEDULE_PREVIEW_FILTER")
            .is_ok_and(|filter| !filter.split(',').any(|part| name == part))
        {
            continue;
        }
        app.preferences.theme = mode;
        app.preferences.widget = Default::default();
        app.preferences.widget_locked = !name.starts_with("desktopcal-");
        app.page = if name.starts_with("settings-") {
            Page::Settings
        } else {
            Page::Month
        };
        app.widget_ui.jump = if name.starts_with("jump-") {
            Some(crate::month_jump::MonthJump::new(
                jump_id,
                crate::month_jump::JumpTarget::Widget,
                if name.contains("year") {
                    crate::month_jump::JumpField::Year
                } else {
                    crate::month_jump::JumpField::Month
                },
                NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            ))
        } else {
            None
        };
        if name == "jump-direct-small" {
            let j = app.widget_ui.jump.as_mut().unwrap();
            j.direct = true;
            j.date = "2027-02-29".into();
            j.error = "1900~9998년의 올바른 날짜를 입력하세요.".into();
        }
        if name == "jump-year-boundary" {
            let j = app.widget_ui.jump.as_mut().unwrap();
            j.year = 9998;
            j.decade = 9990;
        }
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
        if (name.starts_with("quick-") && name != "quick-list") || name == "diary-editor" {
            let mut editor = Editor::new(Form::event(app.quick_events[0].clone()), 1);
            editor.form.title.push_str(" 수정 중");
            if name == "quick-confirm-small" {
                editor.request_close(CloseIntent::Quick);
            }
            app.editor = Some(editor);
            app.editor_in_quick = name.starts_with("quick-");
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

#[test]
fn diary_dates_select_once_and_create_new_event_on_double_click() {
    for weekly in [false, true] {
        let mut app = fixture();
        app.month = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        app.week = NaiveDate::from_ymd_opt(2026, 9, 27).unwrap();
        let expected = "2026-09-27";
        let size = Size::new(960., 640.);
        let bounds = Rectangle::new(Point::ORIGIN, size);
        let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
        let mut element = if weekly {
            crate::views::week(&app)
        } else {
            crate::shell::calendar(&app)
        };
        let mut tree = Tree::new(element.as_widget());
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        let point = if weekly {
            Point::new(275., 30.)
        } else {
            Point::new(60., 56.)
        };
        let mut messages = Vec::new();
        for click in 0..2 {
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
            if click == 0 {
                assert!(
                    messages.iter().any(
                        |m| matches!(m, Message::SelectDay(date) if date.to_string() == expected)
                    ),
                    "single click must select, weekly={weekly}: {messages:?}"
                );
                assert!(!messages.iter().any(|m| matches!(m, Message::New(..))));
                messages.clear();
            }
        }
        assert_eq!(
            messages
                .iter()
                .filter(|m| matches!(m, Message::New(Kind::Event, Some(date)) if date == expected))
                .count(),
            1,
            "double click must create once, weekly={weekly}: {messages:?}"
        );
        drop(element);
        for message in messages {
            let _ = app.update(message);
        }
        let form = &app.editor.as_ref().unwrap().form;
        assert_eq!(form.date, expected);
        assert!(form.id.is_none());
        assert!(!app.editor_in_quick);
    }
}

#[test]
fn diary_week_event_controls_do_not_trigger_date_creation() {
    let mut app = fixture();
    app.week = NaiveDate::from_ymd_opt(2026, 9, 27).unwrap();
    let mut form = Form::new(Kind::Event, app.week.to_string(), None);
    form.title = "기존 일정".into();
    form.save(app.store.as_ref().unwrap()).unwrap();
    app.page = Page::Week;
    app.snapshot = worker::snapshot(app.store.as_ref().unwrap(), &app.scope()).unwrap();
    let size = Size::new(960., 640.);
    let bounds = Rectangle::new(Point::ORIGIN, size);
    let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
    let mut element = crate::views::week(&app);
    let mut tree = Tree::new(element.as_widget());
    let node = element.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, size),
    );
    let mut messages = Vec::new();
    for _ in 0..2 {
        for event in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            element.as_widget_mut().update(
                &mut tree,
                &iced::Event::Mouse(event),
                layout::Layout::new(&node),
                mouse::Cursor::Available(Point::new(280., 120.)),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut iced::advanced::Shell::new(&mut messages),
                &bounds,
            );
        }
    }
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::ToggleEvent(_))),
        "{messages:?}"
    );
    assert!(!messages.iter().any(|m| matches!(m, Message::New(..))));
}

#[test]
fn widget_and_diary_event_forms_share_panel_geometry() {
    use iced::advanced::widget::{
        operation::{Focusable, TextInput},
        Id, Operation,
    };
    struct InputBounds(Option<Rectangle>, Option<Rectangle>);
    impl Operation for InputBounds {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn focusable(&mut self, id: Option<&Id>, bounds: Rectangle, _: &mut dyn Focusable) {
            if id == Some(&Id::new("editor-body")) {
                self.1 = Some(bounds);
            }
        }
        fn text_input(&mut self, id: Option<&Id>, bounds: Rectangle, _: &mut dyn TextInput) {
            if id == Some(&Id::new("editor-title")) {
                self.0 = Some(bounds);
            }
        }
    }
    let mut app = fixture();
    app.quick_window = Some(window::Id::unique());
    let _ = app.update(Message::New(Kind::Event, None));
    let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
    let mut inputs = Vec::new();
    for quick in [false, true] {
        app.editor_in_quick = quick;
        let size = if quick {
            Size::new(650., 560.)
        } else {
            Size::new(1160., 820.)
        };
        let mut element = app.view(if quick {
            app.quick_window.unwrap()
        } else {
            app.manager
        });
        let mut tree = Tree::new(element.as_widget());
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        let mut probe = InputBounds(None, None);
        element.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut probe,
        );
        let mut fields = [
            probe.0.expect("real title input exists"),
            probe.1.expect("real body editor exists"),
        ];
        for field in &mut fields {
            if !quick {
                field.x -= 255.;
                field.y -= 130.;
            }
        }
        inputs.push(fields);
    }
    assert_eq!(
        inputs[0], inputs[1],
        "same form must have identical size and offsets inside its panel"
    );
}

#[test]
fn windows_picker_tiles_choose_year_then_month_without_typing() {
    use crate::month_jump::{JumpField, JumpMessage, JumpTarget};
    use iced::advanced::widget::{Id, Operation};
    struct Labels(Vec<(String, Rectangle)>);
    impl Operation for Labels {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn text(&mut self, _: Option<&Id>, bounds: Rectangle, text: &str) {
            self.0.push((text.into(), bounds));
        }
    }
    fn choose(app: &App, label: &str, expected: &[String]) -> Vec<Message> {
        let size = Size::new(360., 420.);
        let bounds = Rectangle::new(Point::ORIGIN, size);
        let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
        let mut element = app.view(app.widget_ui.jump.as_ref().unwrap().window);
        let mut tree = Tree::new(element.as_widget());
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        let mut labels = Labels(Vec::new());
        element.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut labels,
        );
        for text in expected {
            assert!(
                labels.0.iter().any(|(s, _)| s == text),
                "missing tile {text}"
            );
        }
        let target = labels
            .0
            .iter()
            .find(|(s, _)| s == label)
            .expect("tile exists")
            .1;
        let mut messages = Vec::new();
        for event in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            element.as_widget_mut().update(
                &mut tree,
                &iced::Event::Mouse(event),
                layout::Layout::new(&node),
                mouse::Cursor::Available(target.center()),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut iced::advanced::Shell::new(&mut messages),
                &bounds,
            );
        }
        messages
    }
    for target in [JumpTarget::Widget, JumpTarget::Diary] {
        let mut app = fixture();
        app.month = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        app.widget_month = app.month;
        let _ = app.update(Message::New(Kind::Event, None));
        let _ = app.update(Message::Field(Field::Title("작성 중인 일정".into())));
        let _ = app.jump_message(JumpMessage::Open(target, JumpField::Year));
        for m in choose(
            &app,
            "2028",
            &(2018..=2033).map(|n| n.to_string()).collect::<Vec<_>>(),
        ) {
            let _ = app.update(m);
        }
        assert_eq!(app.month.year(), 2026, "year selection is still a draft");
        assert_eq!(app.widget_month.year(), 2026);
        for m in choose(
            &app,
            "2월",
            &(1..=12).map(|n| format!("{n}월")).collect::<Vec<_>>(),
        ) {
            let _ = app.update(m);
        }
        assert!(app.widget_ui.jump.is_none());
        let chosen = match target {
            JumpTarget::Widget => app.widget_month,
            JumpTarget::Diary => app.month,
        };
        let untouched = match target {
            JumpTarget::Widget => app.month,
            JumpTarget::Diary => app.widget_month,
        };
        assert_eq!(chosen.to_string(), "2028-02-01");
        assert_eq!(untouched.to_string(), "2026-10-01");
        assert_eq!(app.editor.as_ref().unwrap().form.title, "작성 중인 일정");
    }
}

#[test]
fn picker_grid_and_expanded_date_entry_fit_minimum_window() {
    use crate::month_jump::{JumpField, JumpMessage, JumpTarget, PICKER_MIN_SIZE};
    for level in [JumpField::Year, JumpField::Month] {
        for direct in [false, true] {
            let mut app = fixture();
            let _ = app.jump_message(JumpMessage::Open(JumpTarget::Widget, level));
            if direct {
                let _ = app.jump_message(JumpMessage::ToggleDate);
                let _ = app.jump_message(JumpMessage::Date("2027-02-29".into()));
                let _ = app.jump_message(JumpMessage::ApplyDate);
            }
            let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
            let mut element = app.view(app.widget_ui.jump.as_ref().unwrap().window);
            let mut tree = Tree::new(element.as_widget());
            let node = element.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, PICKER_MIN_SIZE),
            );
            fn check(node: &layout::Node, origin: Point) {
                let b = node.bounds();
                let p = Point::new(origin.x + b.x, origin.y + b.y);
                assert!(
                    p.x >= -0.1 && p.y >= -0.1 && p.x + b.width <= 320.1 && p.y + b.height <= 400.1,
                    "out of popup: {p:?} {b:?}"
                );
                for child in node.children() {
                    check(child, p);
                }
            }
            check(&node, Point::ORIGIN);
        }
    }
}

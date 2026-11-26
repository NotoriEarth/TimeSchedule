//! Optional snapshots from the real native renderer, without a desktop session.
use super::*;
use iced::advanced::{graphics::Viewport, layout, mouse, renderer, widget::Tree, Renderer};
use iced::{Point, Rectangle};

fn calendar_record_fixture() -> App {
    let mut app = fixture();
    app.preferences.widget.rolling = false;
    let store = app.store.as_ref().unwrap();
    let project = store
        .save_project(timeschedule_core::diary::ProjectInput {
            id: None,
            title: "TS".into(),
            description: String::new(),
            repo_path: String::new(),
        })
        .unwrap();
    let mut form = Form::new(Kind::Entry, app.selected.to_string(), Some(project.id));
    form.title = "연동기록".into();
    form.body = "양쪽에서 열어야 할 원문".into();
    form.save(store).unwrap();
    app.snapshot = worker::snapshot(store, &app.scope()).unwrap();
    app
}

fn calendar_texts(mut element: Element<'_, Message>, size: Size) -> Vec<String> {
    let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
    let mut tree = Tree::new(element.as_widget());
    let node = element.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, size),
    );
    let mut probe = SettingsProbe::default();
    element
        .as_widget_mut()
        .operate(&mut tree, layout::Layout::new(&node), &renderer, &mut probe);
    probe.texts
}

#[test]
fn widget_displays_record_written_in_diary() {
    let app = calendar_record_fixture();
    let labels = calendar_texts(crate::desktop_widget::view(&app), Size::new(960., 640.));
    // A project's record shows as the project and its count, even for one.
    assert!(labels.iter().any(|s| s == "TS"), "{labels:?}");
    assert!(labels.iter().any(|s| s == "+ 1"), "{labels:?}");
    assert!(
        !labels.iter().any(|s| s.contains("연동기록")),
        "the record title stays in the cards: {labels:?}"
    );
}

#[test]
fn diary_calendar_displays_project_and_its_count() {
    let app = calendar_record_fixture();
    let labels = calendar_texts(crate::shell::calendar(&app), Size::new(1200., 1200.));
    assert!(labels.iter().any(|s| s == "TS"), "{labels:?}");
    assert!(labels.iter().any(|s| s == "+ 1"), "{labels:?}");
    assert!(!labels.iter().any(|s| s.contains("연동기록")), "{labels:?}");
}

#[test]
fn project_line_prefers_the_short_name_and_counts_every_record() {
    let app = calendar_record_fixture();
    let mut day = selected_day(&app).clone();
    day.items[0].project_short_name = "타임".into();
    day.items[0].count = 3;
    for quick in [false, true] {
        let labels = calendar_texts(
            crate::calendar_ui::day_items(&app, Some(&day), 200., 60., 12., quick),
            Size::new(200., 60.),
        );
        assert!(labels.iter().any(|s| s == "타임"), "{labels:?}");
        assert!(labels.iter().any(|s| s == "+ 3"), "{labels:?}");
        assert!(!labels.iter().any(|s| s == "TS" || s.contains("연동기록")));
    }
    // A narrow cell keeps the whole count and cuts the name first.
    let labels = calendar_text_bounds(
        crate::calendar_ui::day_items(&app, Some(&day), 46., 20., 12., true),
        Size::new(46., 20.),
    );
    assert!(labels.iter().any(|(s, _)| s == "+ 3"), "{labels:?}");
}

fn overflowing_calendar_fixture(count: usize) -> App {
    let mut app = calendar_record_fixture();
    for index in 1..count {
        // No project: each record keeps its own line and title.
        let mut form = Form::new(Kind::Entry, app.selected.to_string(), None);
        form.title = format!("기록{index}");
        form.save(app.store.as_ref().unwrap()).unwrap();
    }
    app.snapshot = worker::snapshot(app.store.as_ref().unwrap(), &app.scope()).unwrap();
    app
}

fn calendar_text_bounds(mut element: Element<'_, Message>, size: Size) -> Vec<(String, Rectangle)> {
    #[derive(Default)]
    struct Labels(Vec<(String, Rectangle)>);
    impl iced::advanced::widget::Operation for Labels {
        fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation)) {
            visit(self);
        }
        fn text(&mut self, _: Option<&iced::advanced::widget::Id>, bounds: Rectangle, value: &str) {
            self.0.push((value.to_owned(), bounds));
        }
    }
    let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
    let mut tree = Tree::new(element.as_widget());
    let node = element.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, size),
    );
    let mut probe = Labels::default();
    element
        .as_widget_mut()
        .operate(&mut tree, layout::Layout::new(&node), &renderer, &mut probe);
    probe.0
}

fn selected_day(app: &App) -> &timeschedule_core::calendar::CalendarDay {
    app.snapshot
        .days
        .iter()
        .find(|d| d.date == app.selected.to_string())
        .unwrap()
}

#[test]
fn day_cell_names_hidden_items_instead_of_a_count() {
    let mut app = overflowing_calendar_fixture(5);
    app.preferences.widget.line_gap = 0;
    let day = selected_day(&app);
    assert_eq!(day.items.len(), 5);
    // Three 14.3px lines fit (two with the diary's 1px gaps): two items and
    // the line that names the rest.
    for quick in [false, true] {
        let labels = calendar_text_bounds(
            crate::calendar_ui::day_items(&app, Some(day), 240., 45., 11., quick),
            Size::new(240., 45.),
        );
        for item in &day.items[..2] {
            let shown = crate::calendar_ui::cell_title(item);
            assert!(labels.iter().any(|(s, _)| s == shown), "{labels:?}");
        }
        // The overflow line names a project's records by the project only.
        let rest = day.items[2..]
            .iter()
            .map(crate::calendar_ui::cell_title)
            .collect::<Vec<_>>()
            .join(" · ");
        assert!(rest.ends_with("TS") && !rest.contains("연동기록"), "{rest}");
        let summary = labels
            .iter()
            .find(|(s, _)| s == &rest)
            .unwrap_or_else(|| panic!("hidden titles are named: {labels:?}"))
            .1;
        assert!(
            (summary.y + summary.height - 45.).abs() < 1.,
            "the summary sits on the bottom edge: {summary:?}"
        );
        assert!(!labels.iter().any(|(s, _)| s.ends_with("개 더")));
    }
}

#[test]
fn day_cell_project_record_is_one_line_of_name_then_count() {
    let app = calendar_record_fixture();
    let day = selected_day(&app);
    for quick in [false, true] {
        let labels = calendar_text_bounds(
            crate::calendar_ui::day_items(&app, Some(day), 160., 60., 12., quick),
            Size::new(160., 60.),
        );
        let project = labels.iter().find(|(s, _)| s == "TS").expect("name").1;
        let count = labels.iter().find(|(s, _)| s == "+ 1").expect("count").1;
        assert!((project.y - count.y).abs() < 1., "{project:?} {count:?}");
        assert!(project.x < count.x);
        assert_eq!(labels.len(), 2, "no record title: {labels:?}");
    }
}

#[test]
fn day_cell_record_without_project_has_no_prefix() {
    let app = calendar_record_fixture();
    let mut day = selected_day(&app).clone();
    for item in &mut day.items {
        item.project_id = None;
        item.project_title.clear();
        item.project_short_name.clear();
    }
    let labels = calendar_texts(
        crate::calendar_ui::day_items(&app, Some(&day), 160., 60., 12., true),
        Size::new(160., 60.),
    );
    assert_eq!(labels, ["연동기록"]);
}

#[test]
fn day_cell_wraps_a_cut_title_when_room_remains() {
    let app = calendar_record_fixture();
    let mut day = selected_day(&app).clone();
    let title = "아주 긴 작업 제목이 날짜 칸 하나에 다 들어가지 않는 경우";
    // A record without a project line shows its own title.
    day.items[0].project_id = None;
    day.items[0].title = title.into();
    for quick in [false, true] {
        let labels = calendar_text_bounds(
            crate::calendar_ui::day_items(&app, Some(&day), 150., 100., 12., quick),
            Size::new(150., 100.),
        );
        let lines: Vec<_> = labels.iter().filter(|(s, _)| s != "TS").collect();
        assert_eq!(lines.len(), 2, "{labels:?}");
        let (first, second) = (&lines[0], &lines[1]);
        assert!(second.1.y > first.1.y + 10., "{labels:?}");
        assert!(title.starts_with(first.0.trim_end()), "{labels:?}");
        assert!(!first.0.ends_with('…'));
        assert!(
            title.contains(second.0.trim_end_matches('…').trim()),
            "{labels:?}"
        );
    }
}

#[test]
fn day_cell_keeps_one_line_per_item_when_overflowing() {
    let mut app = overflowing_calendar_fixture(5);
    app.preferences.widget.line_gap = 0;
    let mut day = selected_day(&app).clone();
    day.items[0].title = "아주 긴 작업 제목이 날짜 칸 하나에 다 들어가지 않는 경우".into();
    let labels = calendar_text_bounds(
        crate::calendar_ui::day_items(&app, Some(&day), 150., 45., 11., true),
        Size::new(150., 45.),
    );
    let first = labels
        .iter()
        .filter(|(s, _)| s.starts_with("아주"))
        .collect::<Vec<_>>();
    assert_eq!(first.len(), 1, "{labels:?}");
    assert!(first[0].0.ends_with('…'));
}

#[test]
fn day_cell_shows_completed_line_when_room() {
    let app = calendar_record_fixture();
    let mut day = selected_day(&app).clone();
    day.total += 2;
    day.completed = 2;
    let labels = calendar_texts(
        crate::calendar_ui::day_items(&app, Some(&day), 160., 60., 12., true),
        Size::new(160., 60.),
    );
    assert!(labels.iter().any(|s| s == "TS"), "{labels:?}");
    assert!(labels.iter().any(|s| s == "완료 2"), "{labels:?}");
    day.items.clear();
    day.total = 2;
    let labels = calendar_texts(
        crate::calendar_ui::day_items(&app, Some(&day), 160., 60., 12., true),
        Size::new(160., 60.),
    );
    assert_eq!(labels, ["완료 2"]);
}

#[test]
fn day_cell_keeps_completed_line_before_wrapping_titles() {
    let mut app = calendar_record_fixture();
    app.preferences.widget.line_gap = 1;
    let mut day = selected_day(&app).clone();
    day.items[0].project_id = None;
    day.items[0].title = "아주 긴 작업 제목이 날짜 칸 하나에 다 들어가지 않는 경우".into();
    day.total += 2;
    day.completed = 2;
    // Two 16.9px lines and one gap fit in 40px, three lines do not.
    let labels = calendar_texts(
        crate::calendar_ui::day_items(&app, Some(&day), 150., 40., 13., true),
        Size::new(150., 40.),
    );
    assert!(labels.iter().any(|s| s == "완료 2"), "{labels:?}");
    assert_eq!(
        labels.iter().filter(|s| s.starts_with("아주")).count(),
        1,
        "{labels:?}"
    );
}

#[test]
fn unlocked_widget_resize_grip_covers_no_cell_text() {
    let mut app = fixture();
    app.widget_attached = true;
    app.preferences.widget_locked = false;
    let store = app.store.clone().unwrap();
    let last = *app.widget_days().last().unwrap();
    // Long titles, so the cell's last line runs to its right edge.
    for title in [
        "아침 운동과 산책 일정",
        "점심 약속과 은행 업무",
        "코드 리뷰와 배포 점검",
        "저녁 장보기와 요리 준비",
        "독서 모임 발표 자료 정리",
        "주간 회고와 다음 주 계획",
    ] {
        let mut form = Form::new(Kind::Event, last.to_string(), None);
        form.title = title.into();
        form.save(&store).unwrap();
    }
    app.snapshot = worker::snapshot(&store, &app.scope()).unwrap();
    let size = Size::new(960., 640.);
    let grip = Rectangle::new(
        Point::new(size.width - 26., size.height - 26.),
        Size::new(26., 26.),
    );
    let labels = calendar_text_bounds(crate::desktop_widget::view(&app), size);
    assert!(
        labels
            .iter()
            .any(|(s, _)| s.contains("독서 모임") || s.contains("+")),
        "{labels:?}"
    );
    for (s, b) in labels.iter().filter(|(s, _)| s != "↘") {
        assert!(
            b.intersection(&grip).is_none(),
            "{s:?} {b:?} under the grip"
        );
    }
}

#[test]
fn short_unlocked_widget_still_shows_a_line_per_cell() {
    // A six-week month in a short widget with small text: the grip's room
    // must come out of the scrollable minimum, not out of the cells.
    for locked in [true, false] {
        let mut app = fixture();
        app.widget_attached = true;
        app.preferences.widget_locked = locked;
        app.preferences.widget.event_size = 9;
        app.widget_month = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        assert_eq!(app.widget_days().len(), 42, "May 2026 spans six weeks");
        let store = app.store.clone().unwrap();
        let mut form = Form::new(Kind::Event, "2026-05-15".into(), None);
        form.title = "회의".into();
        form.save(&store).unwrap();
        app.snapshot = worker::snapshot(&store, &app.scope()).unwrap();
        let labels = calendar_texts(crate::desktop_widget::view(&app), Size::new(960., 360.));
        assert!(
            labels.iter().any(|s| s == "회의"),
            "locked={locked}: {labels:?}"
        );
    }
}

#[test]
fn settings_switch_stops_and_restarts_authenticated_mcp() {
    // Give this test its own environment and an ephemeral listener port.
    if std::env::var("TIMESCHEDULE_MCP_PORT").as_deref() != Ok("0") {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::tests::visual::settings_switch_stops_and_restarts_authenticated_mcp",
            ])
            .env("TIMESCHEDULE_MCP_PORT", "0")
            .env("TIMESCHEDULE_MCP_DISABLED", "1")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let dir = crate::record_inbox::tests::TestDir::new();
    let mut app = fixture();
    app.settings_section = crate::app::SettingsSection::Mcp;
    app.data_dir = dir.0.clone();
    let _ = app.update(Message::SetMcpEnabled(false));
    assert!(
        !dir.0.join("mcp-power.json").exists(),
        "a secondary window cannot switch power"
    );
    app._instance = Some(InstanceLock::acquire(&dir.0).unwrap());
    let click = |app: &mut App, enabled| {
        let size = Size::new(1000., 1000.);
        let label = if enabled { "MCP 켜기" } else { "MCP 끄기" };
        let target = calendar_text_bounds(crate::views::settings(app), size)
            .into_iter()
            .find(|(s, _)| s == label)
            .unwrap()
            .1;
        let messages = click_text(crate::views::settings(app), size, target);
        assert!(
            matches!(messages.as_slice(), [Message::SetMcpEnabled(value)] if *value == enabled)
        );
        let _ = app.update(Message::SetMcpEnabled(enabled));
        assert_eq!(
            crate::mcp_power::Power::initialize(&app.data_dir)
                .unwrap()
                .enabled,
            enabled
        );
    };
    click(&mut app, true);
    assert!(crate::mcp_connection::probe(&dir.0).is_ok());
    let endpoint = crate::mcp_connection::endpoint(&app.mcp_endpoint().unwrap()).unwrap();
    let original_token = crate::mcp::read_token(&dir.0).unwrap();
    click(&mut app, false);
    assert!(app.mcp_endpoint().is_none());
    assert!(!dir.0.join("mcp-status.json").exists());
    assert!(std::net::TcpStream::connect(endpoint).is_err());
    click(&mut app, true);
    assert!(crate::mcp_connection::probe(&dir.0).is_ok());
    assert!(crate::mcp::read_token(&dir.0).unwrap() == original_token);
    // A token file that cannot be read is replaced at power-on. The warning opens the
    // advanced part even though the owner had folded it.
    let _ = app.update(Message::McpAdvanced(false));
    click(&mut app, false);
    std::fs::write(dir.0.join("mcp-token.dpapi"), b"copied from another PC").unwrap();
    click(&mut app, true);
    assert_eq!(
        app.mcp_setup.token,
        crate::mcp_setup_ui::TokenPanel::Recovered
    );
    assert_eq!(app.mcp_advanced, Some(true));
    let labels = calendar_texts(crate::views::settings(&app), Size::new(1000., 2400.));
    assert!(labels.iter().any(|s| s == "인증 토큰"), "{labels:?}");
}

fn click_text(mut element: Element<'_, Message>, size: Size, target: Rectangle) -> Vec<Message> {
    let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
    let mut tree = Tree::new(element.as_widget());
    let node = element.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, size),
    );
    let bounds = Rectangle::new(Point::ORIGIN, size);
    let mut messages = Vec::new();
    for press in [true, false] {
        let event = iced::Event::Mouse(if press {
            mouse::Event::ButtonPressed(mouse::Button::Left)
        } else {
            mouse::Event::ButtonReleased(mouse::Button::Left)
        });
        element.as_widget_mut().update(
            &mut tree,
            &event,
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

#[test]
fn editor_year_can_be_opened_and_selected_with_the_mouse() {
    let mut app = fixture();
    let _ = app.open_form(Form::new(Kind::Event, "2026-11-04".into(), None));
    let size = Size::new(550.0, 920.0);
    let labels = calendar_text_bounds(crate::views::editor_view(&app), size);
    let date_label = labels
        .iter()
        .find(|(s, _)| s.starts_with("날짜"))
        .unwrap()
        .1;
    let year = Point::new(date_label.x + 18.0, date_label.y + date_label.height + 20.0);
    for message in click_text(
        crate::views::editor_view(&app),
        size,
        Rectangle::new(year, Size::ZERO),
    ) {
        let _ = app.update(message);
    }
    let options = calendar_text_bounds(crate::views::editor_view(&app), size);
    let next_year = options
        .iter()
        .find(|(label, _)| label.trim_end_matches('년') == "2027")
        .expect("clicking the year should offer 2027")
        .1;
    for message in click_text(crate::views::editor_view(&app), size, next_year) {
        let _ = app.update(message);
    }
    assert_eq!(app.editor.as_ref().unwrap().form.date, "2027-11-04");
}

#[test]
fn date_digits_keep_focus_when_the_picker_closes_in_both_editor_windows() {
    use iced::advanced::widget::{operation::Focusable, Id, Operation};
    use iced::keyboard::{self, key, Key};
    use iced_runtime::UserInterface;

    #[derive(Default)]
    struct DateBounds(Option<Rectangle>);
    impl Operation for DateBounds {
        fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
            visit(self);
        }
        fn focusable(&mut self, id: Option<&Id>, bounds: Rectangle, _: &mut dyn Focusable) {
            if id == Some(&Id::new("editor-date")) {
                self.0 = Some(bounds);
            }
        }
    }
    let digit = |number: &str, code| {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Character(number.into()),
            modified_key: Key::Character(number.into()),
            physical_key: key::Physical::Code(code),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: Some(number.into()),
            repeat: false,
        })
    };

    for quick in [false, true] {
        let mut app = fixture();
        let window = if quick {
            window::Id::unique()
        } else {
            app.manager
        };
        if quick {
            app.quick_window = Some(window);
            app.editor_in_quick = true;
        }
        let _ = app.open_form(Form::new(Kind::Event, "2026-11-04".into(), None));
        let size = if quick {
            crate::views::EDITOR_SIZE
        } else {
            app.manager_size
        };
        let mut renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
        let mut cache = Default::default();
        for (event, expected) in [
            (
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                "2026-11-04",
            ),
            (
                iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                "2026-11-04",
            ),
            (digit("1", key::Code::Digit1), "2026-11-01"),
            (digit("5", key::Code::Digit5), "2026-11-15"),
        ] {
            let mut ui = UserInterface::build(app.view(window), size, cache, &mut renderer);
            let mut probe = DateBounds::default();
            ui.operate(&renderer, &mut probe);
            let bounds = probe.0.expect("the date input must be mounted");
            let point = Point::new(bounds.x + 82., bounds.center_y());
            let mut messages = vec![];
            ui.update(
                &[event],
                mouse::Cursor::Available(point),
                &mut renderer,
                &mut iced::advanced::clipboard::Null,
                &mut messages,
            );
            cache = ui.into_cache();
            for message in messages {
                let _ = app.update(message);
            }
            assert_eq!(
                app.editor.as_ref().unwrap().form.date,
                expected,
                "quick={quick}"
            );
        }
    }
}

#[test]
fn day_cell_summary_opens_the_day_from_its_own_window() {
    let mut app = overflowing_calendar_fixture(5);
    app.preferences.widget.line_gap = 0;
    let day = selected_day(&app);
    let date = app.selected;
    let rest = day.items[2..]
        .iter()
        .map(crate::calendar_ui::cell_title)
        .collect::<Vec<_>>()
        .join(" · ");
    let size = Size::new(240., 45.);
    for widget in [false, true] {
        let summary = calendar_text_bounds(
            crate::calendar_ui::day_items(&app, Some(day), 240., 45., 11., widget),
            size,
        )
        .into_iter()
        .find(|(s, _)| s == &rest)
        .expect("summary line")
        .1;
        let messages = click_text(
            crate::calendar_ui::day_items(&app, Some(day), 240., 45., 11., widget),
            size,
            summary,
        );
        if widget {
            assert!(
                matches!(&messages[..], [Message::WidgetDate(d)] if *d == date),
                "{messages:?}"
            );
        } else {
            assert!(
                matches!(&messages[..], [Message::ManagerDate(d)] if *d == date),
                "{messages:?}"
            );
        }
    }
}

#[test]
fn day_window_is_placed_over_the_window_it_was_opened_from() {
    let mut app = fixture();
    let widget = window::Id::unique();
    app.widget = Some(widget);
    let date = app.selected;
    assert_eq!(app.quick_anchor(), widget, "nothing opened yet: the widget");
    let _ = app.update(Message::ManagerDate(date));
    assert_eq!(app.quick_anchor(), app.manager);
    // Reopening the same day window from the other side moves it there.
    let _ = app.update(Message::WidgetDate(date));
    assert_eq!(app.quick_anchor(), widget);
    let _ = app.update(Message::ManagerDate(date));
    assert_eq!(app.quick_anchor(), app.manager);
    // While an edit is open the window stays where it is.
    let _ = app.update(Message::New(Kind::Event, None));
    assert!(app.editor.is_some());
    let _ = app.update(Message::WidgetDate(date));
    assert_eq!(app.quick_anchor(), app.manager);
}

#[test]
fn day_cell_text_stays_inside_cell() {
    let mut app = overflowing_calendar_fixture(5);
    let mut day = selected_day(&app).clone();
    day.items[1].title = "WWWW 한글 일정 테스트 123456 아주 긴 제목".into();
    day.total += 3;
    day.completed = 3;
    for gap in [0, 3] {
        app.preferences.widget.line_gap = gap;
        for size in [9., 13., 24.] {
            for (width, height) in [(120., 100.), (60., 30.), (40., 10.)] {
                for quick in [false, true] {
                    let labels = calendar_text_bounds(
                        crate::calendar_ui::day_items(&app, Some(&day), width, height, size, quick),
                        Size::new(width, height),
                    );
                    for (s, b) in labels {
                        assert!(
                            b.x + b.width <= width + 0.1 && b.y + b.height <= height + 0.1,
                            "{s:?} {b:?} in {width}x{height} at {size}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn default_diary_and_widget_cells_show_every_record_of_a_busy_day() {
    let app = overflowing_calendar_fixture(3);
    let titles: Vec<_> = selected_day(&app)
        .items
        .iter()
        .map(|i| crate::calendar_ui::cell_title(i).to_string())
        .collect();
    assert_eq!(titles.len(), 3);
    for (view, size) in [
        (
            crate::views::view(&app, app.manager),
            Size::new(1160., 820.),
        ),
        (crate::desktop_widget::view(&app), Size::new(960., 640.)),
    ] {
        let labels = calendar_texts(view, size);
        for title in &titles {
            assert!(labels.contains(title), "{title} hidden: {labels:?}");
        }
        assert!(!labels.iter().any(|s| s.ends_with("개 더")));
    }
}

#[test]
fn widget_has_no_bottom_bar_and_lock_sits_in_the_toolbar() {
    let mut app = calendar_record_fixture();
    for locked in [false, true] {
        app.preferences.widget_locked = locked;
        let labels = calendar_text_bounds(crate::desktop_widget::view(&app), Size::new(960., 640.));
        assert!(
            !labels.iter().any(|(s, _)| s.starts_with("배경 ")),
            "{labels:?}"
        );
        let lock = labels
            .iter()
            .find(|(s, _)| {
                s == if locked {
                    "잠금 해제"
                } else {
                    "위치 잠금"
                }
            })
            .expect("lock control")
            .1;
        let weekday = labels
            .iter()
            .find(|(s, _)| s == "월요일")
            .expect("weekday heading")
            .1;
        assert!(lock.y < weekday.y, "{lock:?} {weekday:?}");
    }
}

#[test]
#[ignore = "manual native overflow previews"]
fn render_calendar_overflow_previews() {
    let output = PathBuf::from(std::env::var_os("TIMESCHEDULE_PREVIEW_DIR").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let mut app = overflowing_calendar_fixture(3);
    // A busy day with timed, all-day and long items, and one that overflows.
    let store = app.store.clone().unwrap();
    for (day, start, title) in [
        (12, "09:00", "팀 주간 회의"),
        (12, "14:00", "치과 예약"),
        (12, "", "장보기"),
        (
            20,
            "10:00",
            "아주 긴 제목의 분기 계획 검토 회의와 후속 정리",
        ),
        (22, "08:30", "출근 전 운동"),
        (22, "12:00", "점심 약속"),
        (22, "15:00", "코드 리뷰"),
        (22, "18:00", "저녁 장보기"),
        (22, "20:00", "독서 모임"),
        (22, "21:00", "주간 회고"),
    ] {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 10, day).unwrap();
        let mut form = Form::new(Kind::Event, date.to_string(), None);
        form.title = title.into();
        form.all_day = start.is_empty();
        if !start.is_empty() {
            form.start = start.into();
            form.end = "23:00".into();
        }
        form.save(&store).unwrap();
    }
    app.snapshot = worker::snapshot(&store, &app.scope()).unwrap();
    for (name, widget, size, theme) in [
        ("diary", false, Size::new(1160., 820.), AppTheme::Midnight),
        (
            "diary-tall",
            false,
            Size::new(1160., 1280.),
            AppTheme::Midnight,
        ),
        ("widget", true, Size::new(960., 640.), AppTheme::Midnight),
        (
            "widget-tall",
            true,
            Size::new(960., 1100.),
            AppTheme::Midnight,
        ),
        (
            "widget-light",
            true,
            Size::new(960., 640.),
            AppTheme::Porcelain,
        ),
        (
            "widget-small",
            true,
            Size::new(640., 420.),
            AppTheme::Midnight,
        ),
    ] {
        app.preferences.theme = theme;
        app.manager_size = size;
        let mut element = if widget {
            crate::desktop_widget::view(&app)
        } else {
            crate::views::view(&app, app.manager)
        };
        let mut renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
        let mut tree = Tree::new(element.as_widget());
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        render_scaled_frame(
            &app,
            &mut element,
            &mut tree,
            &node,
            &mut renderer,
            size,
            &output.join(format!("{name}.png")),
            1.,
        );
    }
}

fn run_message(app: &mut App, message: Message) {
    let task = app.update(message);
    run_task(app, task);
}

fn run_task(app: &mut App, task: Task<Message>) {
    use iced::futures::StreamExt;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut pending = std::collections::VecDeque::from([task]);
            while let Some(task) = pending.pop_front() {
                if let Some(mut stream) = iced_runtime::task::into_stream(task) {
                    while let Some(action) = stream.next().await {
                        if let iced_runtime::Action::Output(message) = action {
                            pending.push_back(app.update(message));
                        }
                    }
                }
            }
        })
        .await
        .expect("native task must finish");
    });
}

#[test]
fn project_panel_receives_experiences_and_exports_markdown_from_its_buttons() {
    use crate::record_inbox::{
        self,
        tests::{sample, TestDir},
    };
    let dir = TestDir::new();
    let mut app = fixture();
    app.portfolio_writer = |prompt| {
        let input: serde_json::Value =
            serde_json::from_str(prompt.lines().last().unwrap()).unwrap();
        let sections: Vec<_> = input["experiences"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| serde_json::json!({"id":e["id"],"paragraphs":["원인과 검증 범위"]}))
            .collect();
        Ok(serde_json::to_string(&sections).unwrap())
    };
    app.data_dir = dir.0.clone();
    let store = app.store.clone().unwrap();
    let fix = sample();
    let mut design = sample();
    design.kind = "design".into();
    design.work_id = uuid::Uuid::new_v4().to_string();
    design.title = "보관 방식 설계".into();
    for record in [&design, &fix, &fix] {
        record_inbox::enqueue(&dir.0, record).unwrap();
    }
    assert_eq!(record_inbox::drain(&dir.0, &store).unwrap().saved, 2);
    let project = store.list_projects().unwrap().remove(0).id;
    run_message(&mut app, Message::Project(Some(project.clone())));
    app.project_tab = crate::app::ProjectTab::Portfolio;
    let click = |app: &mut App, label: &str| {
        let size = Size::new(930., 820.);
        let target = calendar_text_bounds(crate::views::projects(app), size)
            .into_iter()
            .find(|(s, _)| s == label)
            .expect("project control must be visible")
            .1;
        let messages = click_text(crate::views::projects(app), size, target);
        assert!(!messages.is_empty(), "missing click for {label}");
        for message in messages {
            run_message(app, message);
        }
    };
    click(&mut app, "개인 기여 보류");
    assert_eq!(
        app.snapshot.portfolio.as_ref().unwrap().answer.as_deref(),
        Some("")
    );
    // An earlier edition exists; the UI must report the edition actually exported.
    store.export_portfolio(&project, &dir.0).unwrap();
    click(&mut app, "백엔드 포트폴리오 · Markdown 생성");
    let panel_has = |app: &App, label: &str| {
        calendar_text_bounds(
            crate::portfolio_ui::panel(app, app.snapshot.portfolio.as_ref().unwrap()),
            Size::new(930., 820.),
        )
        .iter()
        .any(|(s, _)| s == label)
    };
    assert!(panel_has(&app, "새 v2 생성 완료"));
    let state = app.snapshot.portfolio.as_ref().unwrap();
    assert_eq!(state.experiences, 2);
    let file = dir.0.join(state.last_file.as_ref().unwrap());
    let markdown = std::fs::read_to_string(&file).unwrap();
    assert!(markdown.contains("보관 방식 설계") && markdown.contains("원인과 검증 범위"));
    assert!(file.with_file_name("evidence.md").is_file());
    std::fs::write(&file, "지원자가 수정한 내용").unwrap();
    app.loading = true; // Same-project background refresh, with the previous snapshot still visible.
    click(&mut app, "백엔드 포트폴리오 · Markdown 생성");
    assert!(panel_has(&app, "기존 v2 재사용 완료"));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "지원자가 수정한 내용"
    );
    std::fs::remove_file(file.with_file_name("evidence.md")).unwrap();
    click(&mut app, "백엔드 포트폴리오 · Markdown 생성");
    assert!(app
        .portfolio_feedback
        .as_ref()
        .unwrap()
        .1
        .starts_with("Markdown 생성 실패"));
    assert!(!panel_has(&app, "기존 v2 재사용 완료"));
}

#[test]
fn calendar_routes_create_edit_move_complete_and_delete_the_same_source() {
    use crate::calendar_ui::{message, Action};
    use timeschedule_core::calendar::CalendarSource;
    let mut app = fixture();
    app.preferences.widget.rolling = false;
    app.quick_date = Some(app.selected);
    app.quick_window = Some(window::Id::unique());
    run_message(&mut app, message(Action::New(Kind::Entry, true)));
    assert!(app.editor_in_quick);
    run_message(
        &mut app,
        Message::Field(Field::Title("위젯에서 만든 기록".into())),
    );
    app.editor.as_mut().unwrap().form.body = "같은 원문".into();
    run_message(&mut app, Message::Save);
    let item = app
        .snapshot
        .days
        .iter()
        .flat_map(|d| &d.items)
        .find(|i| i.title == "위젯에서 만든 기록")
        .unwrap()
        .clone();
    assert!(app
        .snapshot
        .widget_days
        .iter()
        .flat_map(|d| &d.items)
        .any(|i| i.id == item.id));
    run_message(
        &mut app,
        message(Action::Open(CalendarSource::Entry, item.id.clone(), false)),
    );
    assert_eq!(app.calendar.detail.as_ref().unwrap().body, "같은 원문");
    run_message(&mut app, message(Action::EditDetail));
    assert!(!app.editor_in_quick);
    let moved = (app.selected + Duration::days(1)).to_string();
    run_message(&mut app, Message::Field(Field::Date(moved.clone())));
    run_message(
        &mut app,
        Message::Field(Field::Title("다이어리 수정".into())),
    );
    run_message(&mut app, Message::Save);
    assert_eq!(
        app.store
            .as_ref()
            .unwrap()
            .get_entry(&item.id)
            .unwrap()
            .date,
        moved
    );
    assert!(app
        .snapshot
        .widget_days
        .iter()
        .find(|d| d.date == moved)
        .unwrap()
        .items
        .iter()
        .any(|i| i.id == item.id && i.title == "다이어리 수정"));
    run_message(
        &mut app,
        message(Action::Open(CalendarSource::Entry, item.id.clone(), true)),
    );
    run_message(&mut app, message(Action::EditDetail));
    run_message(&mut app, Message::Delete);
    run_message(&mut app, Message::ConfirmDelete);
    assert!(app.store.as_ref().unwrap().get_entry(&item.id).is_err());
    assert!(!app
        .snapshot
        .days
        .iter()
        .flat_map(|d| &d.items)
        .any(|i| i.id == item.id));
    run_message(&mut app, message(Action::New(Kind::Event, false)));
    run_message(&mut app, Message::Field(Field::Title("공통 일정".into())));
    run_message(&mut app, Message::Save);
    let id = app
        .snapshot
        .days
        .iter()
        .flat_map(|d| &d.items)
        .find(|i| i.title == "공통 일정")
        .unwrap()
        .id
        .clone();
    run_message(
        &mut app,
        message(Action::Completed(CalendarSource::Event, id, true)),
    );
    assert_eq!(
        app.snapshot
            .widget_days
            .iter()
            .find(|d| d.date == moved)
            .unwrap()
            .completed,
        1
    );
}

#[test]
fn category_add_button_saves_the_entered_name_and_color() {
    use crate::category_ui::{msg, Action};
    let mut app = calendar_record_fixture();
    run_message(&mut app, msg(Action::Open));
    run_message(&mut app, msg(Action::Name("운동".into())));
    run_message(&mut app, msg(Action::Color("#123ABC".into())));
    click_category_control(&mut app, "+ 추가");
    let saved = app.store.as_ref().unwrap().list_categories().unwrap();
    assert!(
        saved
            .iter()
            .any(|c| c.name == "운동" && c.color == "#123ABC"),
        "clicking Add after entering a name must save it; draft={:?}, error={:?}",
        app.categories.name,
        app.categories.error
    );
    assert_eq!(app.snapshot.categories, saved);
    run_message(&mut app, msg(Action::Name("운동".into())));
    click_category_control(&mut app, "+ 추가");
    assert!(!app.categories.error.is_empty());
    assert_eq!(
        app.categories.name, "운동",
        "rejected name must remain editable"
    );
    assert_eq!(app.snapshot.categories, saved);
}

#[test]
fn category_save_button_reaches_the_store_from_the_real_modal() {
    use crate::category_ui::{msg, Action};
    let mut app = calendar_record_fixture();
    run_message(&mut app, msg(Action::Open));
    run_message(&mut app, msg(Action::Name("운동".into())));
    run_message(&mut app, msg(Action::Color("#123ABC".into())));
    click_category_control(&mut app, "카테고리 추가");
    assert!(app.snapshot.categories.iter().any(|c| c.name == "운동"));
}

#[test]
fn category_edit_and_new_draft_controls_keep_existing_categories() {
    use crate::category_ui::{msg, Action};
    let mut app = calendar_record_fixture();
    run_message(&mut app, msg(Action::Open));
    let id = app.snapshot.categories[1].id.clone();
    let count = app.snapshot.categories.len();
    run_message(&mut app, msg(Action::Edit(id.clone())));
    run_message(&mut app, msg(Action::Name("학습 기록".into())));
    click_category_control(&mut app, "변경 저장");
    assert_eq!(app.snapshot.categories.len(), count);
    assert_eq!(
        app.snapshot
            .categories
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .name,
        "학습 기록"
    );
    run_message(&mut app, msg(Action::Edit(id.clone())));
    click_category_control(&mut app, "새로 작성");
    assert!(app.categories.editing.is_none());
    assert!(app.categories.name.is_empty());
    click_category_control(&mut app, "+ 추가");
    assert!(
        !app.categories.error.is_empty(),
        "empty name must explain why nothing was added"
    );
    assert_eq!(app.snapshot.categories.len(), count);
    run_message(&mut app, msg(Action::Name("독서".into())));
    click_category_control(&mut app, "+ 추가");
    assert_eq!(app.snapshot.categories.len(), count + 1);
    assert!(app
        .snapshot
        .categories
        .iter()
        .any(|c| c.name == "독서" && c.id != id));
}

fn click_category_control(app: &mut App, label: &str) {
    click_category_control_at(app, label, 0);
}

fn click_category_control_at(app: &mut App, label: &str, occurrence: usize) {
    let size = Size::new(800., 620.);
    app.manager_size = size;
    let messages = {
        let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
        let mut element = app.view(app.manager);
        let mut tree = Tree::new(element.as_widget());
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        let mut probe = CategoryPositionProbe::default();
        element.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut probe,
        );
        let target = probe
            .0
            .iter()
            .filter(|(s, _)| s == label)
            .nth(occurrence)
            .expect("button label")
            .1
            .center();
        let mut messages = Vec::new();
        for event in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            element.as_widget_mut().update(
                &mut tree,
                &iced::Event::Mouse(event),
                layout::Layout::new(&node),
                mouse::Cursor::Available(target),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut iced::advanced::Shell::new(&mut messages),
                &Rectangle::new(Point::ORIGIN, size),
            );
        }
        messages
    };
    assert!(!messages.is_empty(), "button must dispatch an action");
    for message in messages {
        run_message(app, message);
    }
}

fn category_delete_fixture() -> (
    App,
    timeschedule_core::calendar::Category,
    Vec<timeschedule_core::calendar::EntryEdit>,
) {
    let mut app = calendar_record_fixture();
    let s = app.store.as_ref().unwrap().clone();
    let source = s.save_category(None, "학습", "#123456").unwrap();
    s.save_category(None, "보관", "#ABCDEF").unwrap();
    let mut records = Vec::new();
    for i in 0..3 {
        let mut form = Form::new(Kind::Entry, app.selected.to_string(), None);
        form.title = format!("분류 기록{i}");
        form.body = format!("유지할 원문{i}");
        form.category_id = source.id.clone();
        form.save(&s).unwrap();
    }
    for head in s.calendar_page(&app.selected.to_string(), 0).unwrap().items {
        if head.category_id == source.id {
            records.push(s.calendar_entry(&head.id).unwrap());
        }
    }
    let ids: Vec<_> = s
        .list_categories()
        .unwrap()
        .iter()
        .map(|c| c.id.clone())
        .collect();
    let mut order = ids.clone();
    order.retain(|id| *id != source.id);
    order.insert(0, source.id.clone());
    s.reorder_categories(&ids, &order).unwrap();
    app.snapshot = worker::snapshot(&s, &app.scope()).unwrap();
    run_message(
        &mut app,
        crate::category_ui::msg(crate::category_ui::Action::Open),
    );
    (app, source, records)
}

#[test]
fn category_delete_controls_move_records_and_refresh_both_surfaces() {
    use crate::category_ui::{msg, Action};
    let (mut app, source, records) = category_delete_fixture();
    assert_eq!(records.len(), 3);
    click_category_control(&mut app, "수정");
    assert_eq!(app.categories.editing.as_ref(), Some(&source.id));
    click_category_control(&mut app, "카테고리 삭제");
    assert_eq!(
        app.categories
            .delete_preview
            .as_ref()
            .unwrap()
            .entry_ids
            .len(),
        3
    );
    assert!(app.categories.delete_target.is_none());
    run_message(&mut app, msg(Action::ConfirmDelete));
    assert!(app.snapshot.categories.iter().any(|c| c.id == source.id));
    click_category_control(&mut app, "보관");
    let target = app.categories.delete_target.as_ref().unwrap().id.clone();
    click_category_control(&mut app, "이동 후 삭제");
    assert!(!app.categories.saving);
    assert!(!app.snapshot.categories.iter().any(|c| c.id == source.id));
    for old in records {
        assert_eq!(
            app.store
                .as_ref()
                .unwrap()
                .calendar_entry(&old.entry.id)
                .unwrap()
                .entry,
            old.entry
        );
        for days in [&app.snapshot.days, &app.snapshot.widget_days] {
            let shown = days
                .iter()
                .flat_map(|d| &d.items)
                .find(|item| item.id == old.entry.id)
                .unwrap();
            assert_eq!(shown.category_id, target);
        }
    }
    for element in [
        crate::shell::calendar(&app),
        crate::desktop_widget::view(&app),
    ] {
        let labels = calendar_texts(element, Size::new(1200., 1200.));
        // A record without a project shows only its title; the moved category
        // shows as the line's colour, checked through the snapshot above.
        assert!(
            labels.iter().any(|label| label.starts_with("분류 기록")),
            "{labels:?}"
        );
    }
    assert!(app.categories.notice.contains("3"));
}

#[test]
fn category_rename_control_keeps_id() {
    use crate::category_ui::{msg, Action};
    let (mut app, source, _) = category_delete_fixture();
    click_category_control(&mut app, "수정");
    run_message(&mut app, msg(Action::Name("독서".into())));
    click_category_control(&mut app, "변경 저장");
    let current = app
        .snapshot
        .categories
        .iter()
        .find(|c| c.id == source.id)
        .unwrap();
    assert_eq!(current.name, "독서");
    assert_eq!(current.position, 0);
    assert_eq!(
        app.snapshot.categories,
        app.store.as_ref().unwrap().list_categories().unwrap()
    );
}

#[test]
fn category_delete_cancel_preserves_draft_and_ignores_late_preview() {
    use crate::category_ui::{msg, Action};
    let (mut app, source, _) = category_delete_fixture();
    click_category_control(&mut app, "수정");
    run_message(&mut app, msg(Action::Name("입력 중".into())));
    run_message(&mut app, msg(Action::Color("#987654".into())));
    let task = app.update(msg(Action::RequestDelete));
    let generation = app.categories.delete_generation;
    let preview = app
        .store
        .as_ref()
        .unwrap()
        .preview_category_deletion(&source.id)
        .unwrap();
    let manager = app.manager;
    run_message(&mut app, Message::EscapeWindow(manager));
    assert!(app.categories.open);
    assert_eq!(app.categories.name, "입력 중");
    assert_eq!(app.categories.color, "#987654");
    run_task(&mut app, task);
    assert!(app.categories.delete_preview.is_none());
    run_message(&mut app, Message::EscapeWindow(manager));
    assert!(!app.categories.open);
    run_message(&mut app, msg(Action::Open));
    run_message(
        &mut app,
        msg(Action::DeletePreviewed {
            generation,
            result: Ok(preview),
        }),
    );
    assert!(app.categories.delete_preview.is_none());
    assert!(app
        .snapshot
        .categories
        .iter()
        .any(|c| c.id == source.id && c.name == "학습"));
}

#[test]
fn category_delete_duplicate_confirm_writes_once() {
    use crate::category_ui::{msg, Action};
    let (mut app, _, _) = category_delete_fixture();
    click_category_control(&mut app, "수정");
    click_category_control(&mut app, "카테고리 삭제");
    click_category_control(&mut app, "보관");
    let first = app.update(msg(Action::ConfirmDelete));
    assert!(app.categories.saving);
    assert_eq!(app.update(msg(Action::ConfirmDelete)).units(), 0);
    run_task(&mut app, first);
    assert!(app.categories.error.is_empty());
    assert!(app.categories.notice.contains("3"));
}

#[test]
fn category_delete_preserves_open_record_draft() {
    let (mut app, _, records) = category_delete_fixture();
    let mut form = Form::entry(records[0].entry.clone());
    form.category_id = records[0].display.category_id.clone();
    form.entry_version = Some(records[0].clone());
    form.body = "미저장 초안".into();
    // An editor in another window holds this version while the manager deletes.
    click_category_control(&mut app, "수정");
    click_category_control(&mut app, "카테고리 삭제");
    click_category_control(&mut app, "보관");
    click_category_control(&mut app, "이동 후 삭제");
    assert_eq!(form.body, "미저장 초안");
    assert!(form.save(app.store.as_ref().unwrap()).is_err());
    form.category_id = timeschedule_core::calendar::RECORD_CATEGORY.into();
    assert!(form.save(app.store.as_ref().unwrap()).is_err());
    assert_eq!(
        app.store
            .as_ref()
            .unwrap()
            .get_entry(&records[0].entry.id)
            .unwrap()
            .body,
        records[0].entry.body
    );
}

#[test]
fn category_delete_stale_target_reloads_without_moving_records() {
    use crate::category_ui::{msg, Action};
    let (mut app, source, records) = category_delete_fixture();
    click_category_control(&mut app, "수정");
    run_message(&mut app, msg(Action::Name("미저장 분류 이름".into())));
    click_category_control(&mut app, "카테고리 삭제");
    click_category_control(&mut app, "보관");
    let target = app.categories.delete_target.clone().unwrap();
    app.store
        .as_ref()
        .unwrap()
        .save_category(Some(&target.id), "외부 수정", &target.color)
        .unwrap();
    click_category_control(&mut app, "이동 후 삭제");
    assert!(!app.categories.saving);
    assert!(app.categories.delete_target.is_none());
    assert_eq!(
        app.categories
            .delete_preview
            .as_ref()
            .unwrap()
            .entry_ids
            .len(),
        3
    );
    assert!(!app.categories.error.is_empty());
    assert_eq!(app.categories.name, "미저장 분류 이름");
    for record in records {
        assert_eq!(
            app.store
                .as_ref()
                .unwrap()
                .entry_display(&record.entry.id)
                .unwrap()
                .category_id,
            source.id
        );
    }
    click_category_control(&mut app, "외부 수정");
    click_category_control(&mut app, "이동 후 삭제");
    assert!(!app.snapshot.categories.iter().any(|c| c.id == source.id));
}

#[test]
fn category_preview_completion_survives_quick_editor_confirmation() {
    assert_category_completion_survives_quick_editor_confirmation(false);
}

#[test]
fn category_delete_completion_survives_quick_editor_confirmation() {
    assert_category_completion_survives_quick_editor_confirmation(true);
}

fn assert_category_completion_survives_quick_editor_confirmation(deleting: bool) {
    use crate::category_ui::{msg, Action};
    let (mut app, source, records) = category_delete_fixture();
    click_category_control(&mut app, "수정");
    // Open a real existing-record editor in the quick window while the
    // category manager remains mounted in the main window.
    app.quick_window = Some(window::Id::unique());
    app.quick_date = Some(app.selected);
    let mut form = Form::entry(records[0].entry.clone());
    form.category_id = source.id.clone();
    form.entry_version = Some(records[0].clone());
    app.calendar.detail = Some(form);
    app.calendar.detail_quick = true;
    run_message(
        &mut app,
        crate::calendar_ui::message(crate::calendar_ui::Action::EditDetail),
    );
    assert!(app.editor_in_quick);
    assert!(app.categories.open);
    let pending = if deleting {
        run_message(&mut app, msg(Action::RequestDelete));
        run_message(
            &mut app,
            msg(Action::DeleteTarget(
                timeschedule_core::calendar::RECORD_CATEGORY.into(),
            )),
        );
        app.update(msg(Action::ConfirmDelete))
    } else {
        app.update(msg(Action::RequestDelete))
    };
    run_message(&mut app, Message::Delete);
    assert_eq!(
        app.editor.as_ref().unwrap().confirmation,
        Some(Confirmation::Delete)
    );
    run_task(&mut app, pending);
    assert_eq!(
        app.editor.as_ref().unwrap().confirmation,
        Some(Confirmation::Delete)
    );
    assert_eq!(
        app.editor.as_ref().unwrap().form.body,
        records[0].entry.body
    );
    assert!(
        !app.categories.delete_loading,
        "preview completion was lost"
    );
    assert!(!app.categories.saving, "delete completion was lost");
    if deleting {
        assert!(!app.snapshot.categories.iter().any(|c| c.id == source.id));
        let original = &records[0].entry;
        assert_eq!(
            app.store.as_ref().unwrap().get_entry(&original.id).unwrap(),
            *original
        );
        assert!(app
            .editor
            .as_ref()
            .unwrap()
            .form
            .save(app.store.as_ref().unwrap())
            .is_err());
    } else {
        assert!(app.categories.delete_preview.is_some());
    }
    // User actions stay blocked by the other confirmation.
    let old_target = app.categories.delete_target.clone();
    run_message(
        &mut app,
        msg(Action::DeleteTarget(
            timeschedule_core::calendar::RECORD_CATEGORY.into(),
        )),
    );
    assert_eq!(app.categories.delete_target, old_target);
    run_message(&mut app, Message::Keep);
    run_message(&mut app, msg(Action::Close));
    if app.categories.open {
        run_message(&mut app, msg(Action::Close));
    }
    assert!(!app.categories.open);
}

#[test]
fn category_delete_empty_confirm_and_basic_protection_use_real_controls() {
    use crate::category_ui::{msg, Action};
    let (mut app, _, _) = category_delete_fixture();
    let empty = app
        .snapshot
        .categories
        .iter()
        .find(|c| c.name == "보관")
        .unwrap()
        .id
        .clone();
    run_message(&mut app, msg(Action::Edit(empty.clone())));
    click_category_control(&mut app, "카테고리 삭제");
    assert!(app
        .categories
        .delete_preview
        .as_ref()
        .unwrap()
        .entry_ids
        .is_empty());
    // The confirmation heading has the same text; click the footer button.
    click_category_control_at(&mut app, "카테고리 삭제", 1);
    assert!(!app.snapshot.categories.iter().any(|c| c.id == empty));
    run_message(
        &mut app,
        msg(Action::Edit(
            timeschedule_core::calendar::RECORD_CATEGORY.into(),
        )),
    );
    let labels = calendar_texts(crate::category_ui::view(&app), Size::new(600., 700.));
    assert!(labels
        .iter()
        .any(|s| s == "기본 카테고리는 삭제할 수 없습니다"));
    assert!(!labels.iter().any(|s| s == "카테고리 삭제"));
    run_message(&mut app, msg(Action::Name("기본 기록 새 이름".into())));
    click_category_control(&mut app, "변경 저장");
    assert!(app
        .snapshot
        .categories
        .iter()
        .any(|c| c.id == timeschedule_core::calendar::RECORD_CATEGORY
            && c.name == "기본 기록 새 이름"));
}

#[test]
#[ignore = "manual category delete/rename native renderer previews"]
fn render_category_delete_rename_previews() {
    use crate::category_ui::{msg, Action};
    let output = PathBuf::from(std::env::var_os("TIMESCHEDULE_PREVIEW_DIR").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    for (state_name, state) in [("edit", 0), ("move", 1), ("empty", 2), ("basic", 3)] {
        for (size_name, size) in [
            ("small", Size::new(800., 620.)),
            ("wide", Size::new(1160., 820.)),
        ] {
            let (mut app, source, _) = category_delete_fixture();
            let long = "프로젝트 기록과 학습 내용을 함께 관리하는 카테고리 이름";
            app.store
                .as_ref()
                .unwrap()
                .save_category(Some(&source.id), long, &source.color)
                .unwrap();
            app.snapshot = worker::snapshot(app.store.as_ref().unwrap(), &app.scope()).unwrap();
            let edit_id = if state == 2 {
                app.snapshot
                    .categories
                    .iter()
                    .find(|c| c.name == "보관")
                    .unwrap()
                    .id
                    .clone()
            } else if state == 3 {
                timeschedule_core::calendar::RECORD_CATEGORY.into()
            } else {
                source.id.clone()
            };
            run_message(&mut app, msg(Action::Edit(edit_id)));
            if state == 1 || state == 2 {
                run_message(&mut app, msg(Action::RequestDelete));
            }
            if state == 1 {
                run_message(
                    &mut app,
                    msg(Action::DeleteTarget(
                        timeschedule_core::calendar::RECORD_CATEGORY.into(),
                    )),
                );
            }
            for mode in [AppTheme::Midnight, AppTheme::Porcelain] {
                app.preferences.theme = mode;
                app.manager_size = size;
                let mut element = app.view(app.manager);
                let mut renderer =
                    iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
                let mut tree = Tree::new(element.as_widget());
                let node = element.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, size),
                );
                render_scaled_frame(
                    &app,
                    &mut element,
                    &mut tree,
                    &node,
                    &mut renderer,
                    size,
                    &output.join(format!("{state_name}-{size_name}-{mode:?}.png")),
                    1.,
                );
            }
        }
    }
}

#[test]
fn category_drag_writes_only_on_drop_and_refreshes_both_surfaces() {
    use crate::category_ui::{msg, Action};
    let mut app = calendar_record_fixture();
    let before = app.store.as_ref().unwrap().list_categories().unwrap();
    run_message(&mut app, msg(Action::Open));
    assert_eq!(
        app.store.as_ref().unwrap().list_categories().unwrap(),
        before
    );
    run_message(
        &mut app,
        msg(Action::Reorder {
            expected: before.iter().map(|c| c.id.clone()).collect(),
            order: before.iter().rev().map(|c| c.id.clone()).collect(),
        }),
    );
    assert_eq!(app.snapshot.categories[0].id, before[1].id);
    run_message(&mut app, msg(Action::Name("운동".into())));
    run_message(&mut app, msg(Action::Color("#123ABC".into())));
    run_message(&mut app, msg(Action::Save));
    assert_eq!(app.snapshot.categories.last().unwrap().name, "운동");
    assert_eq!(app.snapshot.categories.last().unwrap().color, "#123ABC");
}

#[test]
fn category_drag_handle_follows_pointer_before_any_save() {
    use crate::category_ui::{msg, Action};
    let mut app = calendar_record_fixture();
    run_message(&mut app, msg(Action::Open));
    let before = app.store.as_ref().unwrap().list_categories().unwrap();
    let size = Size::new(520., 650.);
    let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
    let mut tree = Tree::empty();
    let initial = calendar_text_bounds(crate::category_ui::view(&app), size);
    let handle = initial
        .iter()
        .filter(|(s, _)| s == "⠿")
        .nth(1)
        .unwrap()
        .1
        .center();
    let start = initial.iter().find(|(s, _)| s == "기록").unwrap().1.y;
    for (event, point) in [
        (mouse::Event::ButtonPressed(mouse::Button::Left), handle),
        (
            mouse::Event::CursorMoved {
                position: handle - iced::Vector::new(0., 20.),
            },
            handle - iced::Vector::new(0., 20.),
        ),
    ] {
        let messages = {
            let mut element = crate::category_ui::view(&app);
            tree.diff(element.as_widget());
            let node = element.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, size),
            );
            let mut messages = Vec::new();
            element.as_widget_mut().update(
                &mut tree,
                &iced::Event::Mouse(event),
                layout::Layout::new(&node),
                mouse::Cursor::Available(point),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut iced::advanced::Shell::new(&mut messages),
                &Rectangle::new(Point::ORIGIN, size),
            );
            messages
        };
        for message in messages {
            run_message(&mut app, message);
        }
    }
    let mut element = crate::category_ui::view(&app);
    tree.diff(element.as_widget());
    let node = element.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, size),
    );
    let mut probe = CategoryPositionProbe::default();
    element
        .as_widget_mut()
        .operate(&mut tree, layout::Layout::new(&node), &renderer, &mut probe);
    let moved = probe.0.iter().find(|(s, _)| s == "기록").unwrap().1.y;
    assert!(
        (moved - (start - 20.)).abs() < 0.5,
        "grabbed row should follow cursor immediately: {start} -> {moved}"
    );
    assert_eq!(
        app.store.as_ref().unwrap().list_categories().unwrap(),
        before
    );
}

#[derive(Default)]
struct CategoryPositionProbe(Vec<(String, Rectangle)>);
impl iced::advanced::widget::Operation for CategoryPositionProbe {
    fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation)) {
        visit(self);
    }
    fn text(&mut self, _: Option<&iced::advanced::widget::Id>, bounds: Rectangle, value: &str) {
        self.0.push((value.into(), bounds));
    }
}

#[test]
fn category_drag_stale_save_preserves_drafts_and_external_categories() {
    use crate::category_ui::{msg, Action};
    let mut app = calendar_record_fixture();
    run_message(&mut app, msg(Action::Open));
    let expected: Vec<_> = app
        .snapshot
        .categories
        .iter()
        .map(|c| c.id.clone())
        .collect();
    run_message(&mut app, msg(Action::Edit(expected[0].clone())));
    run_message(&mut app, msg(Action::Name("미저장 분류 이름".into())));
    let store = app.store.as_ref().unwrap();
    store.save_category(None, "외부 추가", "#123456").unwrap();
    let external = store.list_categories().unwrap();
    run_message(
        &mut app,
        msg(Action::Reorder {
            expected: expected.clone(),
            order: expected.iter().rev().cloned().collect(),
        }),
    );
    assert!(!app.categories.error.is_empty());
    assert!(app.categories.pending_order.is_none());
    assert!(!app.categories.saving);
    assert_eq!(app.categories.name, "미저장 분류 이름");
    assert_eq!(
        app.store.as_ref().unwrap().list_categories().unwrap(),
        external
    );
    assert_eq!(
        app.snapshot.categories, external,
        "a rejected reorder must reload the latest categories without losing the draft"
    );
    let refreshed: Vec<_> = external.iter().map(|c| c.id.clone()).collect();
    let retry_order: Vec<_> = refreshed.iter().rev().cloned().collect();
    run_message(
        &mut app,
        msg(Action::Reorder {
            expected: refreshed,
            order: retry_order.clone(),
        }),
    );
    assert!(app.categories.error.is_empty());
    assert_eq!(app.categories.name, "미저장 분류 이름");
    assert_eq!(
        app.snapshot
            .categories
            .iter()
            .map(|c| c.id.clone())
            .collect::<Vec<_>>(),
        retry_order
    );
}

#[test]
#[ignore = "manual native category drag animation frames"]
fn render_category_drag_motion_previews() {
    use crate::category_ui::{msg, Action};
    let output = PathBuf::from(std::env::var_os("TIMESCHEDULE_PREVIEW_DIR").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    for size in [Size::new(800., 620.), Size::new(1160., 820.)] {
        let mut app = calendar_record_fixture();
        app.manager_size = size;
        app.store
            .as_ref()
            .unwrap()
            .save_category(None, "학습 일지와 기타 프로젝트 기록", "#D9AD65")
            .unwrap();
        app.snapshot = worker::snapshot(app.store.as_ref().unwrap(), &app.scope()).unwrap();
        run_message(&mut app, msg(Action::Open));
        let mut tree = Tree::empty();
        let mut renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
        let mut point = Point::ORIGIN;
        let mut target = Point::ORIGIN;
        let mut time = std::time::Instant::now() + std::time::Duration::from_secs(1);
        for stage in 0..6 {
            let messages = {
                let mut element = crate::views::view(&app, app.manager);
                tree.diff(element.as_widget());
                let node = element.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, size),
                );
                if stage == 0 {
                    let mut probe = CategoryPositionProbe::default();
                    element.as_widget_mut().operate(
                        &mut tree,
                        layout::Layout::new(&node),
                        &renderer,
                        &mut probe,
                    );
                    let handles: Vec<_> = probe
                        .0
                        .iter()
                        .filter(|(s, _)| s == "⠿")
                        .map(|(_, b)| b.center())
                        .collect();
                    point = handles[1];
                    target = handles[0] - iced::Vector::new(0., 6.);
                }
                let event = match stage {
                    1 => iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                    2 => {
                        point = target;
                        iced::Event::Mouse(mouse::Event::CursorMoved { position: point })
                    }
                    4 => iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                    _ => {
                        time += std::time::Duration::from_millis(if stage == 5 { 150 } else { 75 });
                        iced::Event::Window(window::Event::RedrawRequested(time))
                    }
                };
                let mut messages = Vec::new();
                element.as_widget_mut().update(
                    &mut tree,
                    &event,
                    layout::Layout::new(&node),
                    mouse::Cursor::Available(point),
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut iced::advanced::Shell::new(&mut messages),
                    &Rectangle::new(Point::ORIGIN, size),
                );
                render_scaled_frame(
                    &app,
                    &mut element,
                    &mut tree,
                    &node,
                    &mut renderer,
                    size,
                    &output.join(format!("category-{}-{stage}.png", size.width as u32)),
                    1.,
                );
                messages
            };
            for message in messages {
                run_message(&mut app, message);
            }
        }
    }
}

#[test]
fn clicking_widget_record_opens_its_day_before_loading_detail() {
    use crate::calendar_ui::{message, Action};
    let mut app = calendar_record_fixture();
    let item = app
        .snapshot
        .widget_days
        .iter()
        .flat_map(|d| &d.items)
        .next()
        .unwrap()
        .clone();
    assert!(app.quick_window.is_none());
    let _ = app.update(message(Action::Open(item.source, item.id, true)));
    assert!(app.quick_window.is_some());
    assert_eq!(app.quick_date.unwrap().to_string(), item.date);
}

#[test]
fn incoming_records_refresh_open_day_and_detail() {
    use crate::calendar_ui::{message, Action};
    let mut app = calendar_record_fixture();
    app.quick_window = Some(window::Id::unique());
    app.quick_date = Some(app.selected);
    let item = app
        .snapshot
        .days
        .iter()
        .flat_map(|d| &d.items)
        .next()
        .unwrap()
        .clone();
    run_message(
        &mut app,
        message(Action::Open(item.source, item.id.clone(), true)),
    );
    let store = app.store.as_ref().unwrap();
    let mut changed =
        worker::hydrate_form(store, Form::entry(store.get_entry(&item.id).unwrap())).unwrap();
    changed.title = "외부 수정 반영".into();
    changed.save(store).unwrap();
    let mut added = Form::new(Kind::Entry, app.selected.to_string(), None);
    added.title = "새 수신 기록".into();
    added.save(store).unwrap();
    app.records_processing = true;
    run_message(
        &mut app,
        Message::RecordsDrained(Ok(crate::record_inbox::DrainReport {
            saved: 1,
            ..Default::default()
        })),
    );
    assert_eq!(app.calendar.quick.total, 2, "open day must refresh");
    assert_eq!(
        app.calendar.detail.as_ref().unwrap().title,
        "외부 수정 반영"
    );
    app.store.as_ref().unwrap().delete_entry(&item.id).unwrap();
    run_message(&mut app, Message::Load);
    assert!(app.calendar.detail.is_none());
}

#[test]
fn category_reorder_preserves_unsaved_name_and_color() {
    use crate::category_ui::{msg, Action};
    let mut app = calendar_record_fixture();
    let id = app.snapshot.categories[1].id.clone();
    run_message(&mut app, msg(Action::Open));
    run_message(&mut app, msg(Action::Edit(id.clone())));
    run_message(&mut app, msg(Action::Name("미저장 이름".into())));
    run_message(&mut app, msg(Action::Color("#123456".into())));
    run_message(&mut app, msg(Action::Move(id.clone(), -1)));
    assert_eq!(app.categories.editing, Some(id));
    assert_eq!(app.categories.name, "미저장 이름");
    assert_eq!(app.categories.color, "#123456");
}

#[test]
fn shared_calendar_rows_fit_real_narrow_width_with_visible_ellipsis() {
    let app = calendar_record_fixture();
    let mut item = app
        .snapshot
        .days
        .iter()
        .flat_map(|d| &d.items)
        .next()
        .unwrap()
        .clone();
    item.project_title = "아주 긴 프로젝트명 WWWWWWW 한글 가독성 확인 반복".into();
    item.title = "아주 긴 작업 제목 WWWWWWW 날짜 목록에서 말줄임표 확인".into();
    for (actual, quick) in [(318.0, true), (120.0, false)] {
        let labels = calendar_texts(
            crate::calendar_ui::item_view(&app, &item, 14.0, quick),
            Size::new(actual, 70.),
        );
        for label in labels.iter().filter(|s| s.starts_with("아주")) {
            assert!(label.ends_with('…'));
            assert!(
                crate::text_layout::text_width_with_font(
                    label,
                    14.0,
                    iced::Font::with_name("Malgun Gothic")
                ) <= actual - 24.0,
                "{label} overflows {actual}"
            );
        }
    }
}

#[test]
#[ignore = "manual category/calendar native renderer previews"]
fn render_calendar_category_previews() {
    let output = PathBuf::from(std::env::var_os("TIMESCHEDULE_PREVIEW_DIR").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let mut app = calendar_record_fixture();
    for (i, title) in ["회의 준비", "견적 정리"].iter().enumerate() {
        let mut f = Form::new(Kind::Event, app.selected.to_string(), None);
        f.title = title.to_string();
        f.completed = i == 1;
        f.save(app.store.as_ref().unwrap()).unwrap();
    }
    let mut long = Form::new(
        Kind::Entry,
        app.selected.to_string(),
        Some(app.snapshot.projects[0].id.clone()),
    );
    long.title = "아주 긴 제목 WWWWWWW 한글 날짜 목록과 좁은 주간 화면의 말줄임표 확인".into();
    long.save(app.store.as_ref().unwrap()).unwrap();
    app.snapshot = worker::snapshot(app.store.as_ref().unwrap(), &app.scope()).unwrap();
    app.calendar.quick = app
        .store
        .as_ref()
        .unwrap()
        .calendar_section(&app.selected.to_string(), 0, false)
        .unwrap();
    app.quick_date = Some(app.selected);
    for (name, kind, size) in [
        ("diary-wide", 0, Size::new(1160., 820.)),
        ("diary-small", 0, Size::new(800., 620.)),
        ("widget", 1, Size::new(960., 640.)),
        ("categories", 2, Size::new(800., 620.)),
        ("record-editor", 3, Size::new(800., 620.)),
        ("quick-small", 4, Size::new(350., 330.)),
        ("week-small", 5, Size::new(800., 620.)),
    ] {
        for mode in [AppTheme::Midnight, AppTheme::Porcelain] {
            app.preferences.theme = mode;
            app.manager_size = size;
            app.categories.open = kind == 2;
            app.editor = None;
            app.page = if kind == 5 { Page::Week } else { Page::Month };
            app.week = app.selected - Duration::days(2);
            if kind == 2 {
                app.categories.color = "#82A8D8".into();
            }
            if kind == 3 {
                app.editor = Some(Editor::new(
                    Form::new(Kind::Entry, app.selected.to_string(), None),
                    1,
                ));
            }
            let mut element = if kind == 1 {
                crate::desktop_widget::view(&app)
            } else if kind == 4 {
                crate::quick_editor::view(&app)
            } else {
                crate::views::view(&app, app.manager)
            };
            let mut renderer =
                iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
            let mut tree = Tree::new(element.as_widget());
            let node = element.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, size),
            );
            for scale in [1.0, 1.5, 2.0] {
                render_scaled_frame(
                    &app,
                    &mut element,
                    &mut tree,
                    &node,
                    &mut renderer,
                    size,
                    &output.join(format!("{name}-{mode:?}-{scale}.png")),
                    scale,
                );
            }
        }
    }
}

#[path = "scroll_latency.rs"]
mod scroll_latency;

#[derive(Default)]
struct SettingsProbe {
    set_scroll: Option<f32>,
    set_scroll_index: usize,
    texts: Vec<String>,
    scrolls: Vec<(Rectangle, Rectangle, iced::Vector)>,
    widget_status: Option<usize>,
    controls: Vec<(String, Rectangle)>,
}
impl iced::advanced::widget::Operation for SettingsProbe {
    fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation)) {
        visit(self);
    }
    fn scrollable(
        &mut self,
        id: Option<&iced::advanced::widget::Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: iced::Vector,
        state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        if id == Some(&iced::advanced::widget::Id::new("widget-connection-status")) {
            self.widget_status = Some(self.scrolls.len());
        }
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
            "등록 미리보기",
            "등록 적용",
            "연결 검사",
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
    render_scaled_frame(app, element, tree, node, renderer, size, output, 1.0);
}

fn render_scaled_frame(
    app: &App,
    element: &mut Element<'_, Message>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &mut iced::Renderer,
    size: Size,
    output: &std::path::Path,
    scale: f32,
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
    let physical = Size::new((size.width * scale) as u32, (size.height * scale) as u32);
    let mut pixels = tiny_skia::Pixmap::new(physical.width, physical.height).unwrap();
    renderer.draw(
        &mut pixels.as_mut(),
        &mut tiny_skia::Mask::new(physical.width, physical.height).unwrap(),
        &Viewport::with_physical_size(physical, scale),
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
        physical.width,
        physical.height,
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
fn connection_board_keeps_its_height_and_names_each_state() {
    use crate::mcp_clients::{Registration as R, Seen};
    use chrono::TimeZone;
    let now = Local.with_ymd_and_hms(2026, 11, 16, 15, 30, 0).unwrap();
    let seen = |name: &str, minutes: i64| Seen {
        name: name.into(),
        title: None,
        version: "1.0".into(),
        last_seen: now - Duration::minutes(minutes),
    };
    let cases: Vec<(&str, bool, [R; 3], bool, Vec<Seen>, Vec<&str>)> = vec![
        (
            "server-off",
            false,
            [R::Current, R::Current, R::Current],
            false,
            vec![seen("claude-code", 1)],
            vec!["서버 꺼짐"],
        ),
        (
            "checking",
            true,
            [R::Unchecked, R::Unchecked, R::Unchecked],
            true,
            vec![],
            vec!["확인 중"],
        ),
        (
            "mixed",
            true,
            [R::Current, R::Missing, R::Outdated],
            false,
            vec![seen("codex-mcp-client", 3)],
            vec![
                "연결됨",
                "3분 전",
                "미등록",
                "등록 준비",
                "재등록 필요",
                "다시 등록",
            ],
        ),
        (
            "registered",
            true,
            [
                R::Current,
                R::Current,
                R::Error("설정 파일의 JSON 형식이 잘못되었습니다.".into()),
            ],
            false,
            vec![seen("claude-code", 90), seen("cursor-vscode", 1)],
            vec![
                "등록됨",
                "다시 시작",
                "오늘 14:00",
                "확인 실패",
                "cursor-vscode",
            ],
        ),
    ];
    for mode in [AppTheme::Midnight, AppTheme::Porcelain] {
        for width in [800., 1160.] {
            let mut app = fixture();
            app.preferences.theme = mode;
            let mut renderer =
                iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
            let mut height = None;
            for (name, running, registrations, checking, log, expected) in &cases {
                app.mcp_setup.registrations = registrations.clone();
                app.mcp_setup.checking = *checking;
                let mut element = crate::mcp_setup_ui::board(&app.mcp_setup, *running, log, now);
                let mut tree = Tree::new(element.as_widget());
                let node = element.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, Size::new(width, 2000.)),
                );
                let mut probe = SettingsProbe::default();
                element.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut probe,
                );
                let all = probe.texts.join(" | ");
                for text in expected {
                    assert!(all.contains(text), "{name}: missing {text:?} in {all}");
                }
                for app_name in ["Codex · ChatGPT Desktop", "Claude Code", "Claude Desktop"] {
                    assert!(all.contains(app_name), "{name}: row {app_name} missing");
                }
                let measured = node.size().height;
                match height {
                    None => height = Some(measured),
                    Some(first) => assert_eq!(
                        measured, first,
                        "{name}: the board changed height, moving every control below it"
                    ),
                }
                if let Some(output) = std::env::var_os("TIMESCHEDULE_MCP_SETUP_PREVIEW_DIR") {
                    let output = PathBuf::from(output);
                    std::fs::create_dir_all(&output).unwrap();
                    render_settings_frame(
                        &app,
                        &mut element,
                        &mut tree,
                        &node,
                        &mut renderer,
                        Size::new(width, measured),
                        &output.join(format!("board-{mode:?}-{width}-{name}.png")),
                    );
                }
            }
        }
    }
}

type TokenPanelCase = (
    &'static str,
    crate::mcp_setup_ui::TokenPanel,
    bool,
    Vec<&'static str>,
);
fn token_panel_cases() -> Vec<TokenPanelCase> {
    use crate::mcp_setup_ui::TokenPanel as T;
    vec![
        (
            "idle",
            T::Idle,
            false,
            vec!["인증 토큰", "토큰 재발급", "유출"],
        ),
        (
            "confirming",
            T::Confirming,
            false,
            vec!["취소", "재발급 확인", "즉시 거절", "환경변수"],
        ),
        ("working", T::Working, true, vec!["토큰 재발급", "저장하고"]),
        (
            "reissued",
            T::Reissued { running: true },
            false,
            vec!["이전 토큰은 거절", "다시 연결", "환경변수"],
        ),
        (
            "reissued-off",
            T::Reissued { running: false },
            false,
            vec!["MCP 켜기", "새 토큰으로 연결"],
        ),
        (
            "failed",
            T::Failed(
                "MCP 인증 파일을 교체하지 못했습니다. 다른 프로그램이 파일을 사용 중인지 확인하세요. 이전 토큰은 그대로입니다."
                    .into(),
            ),
            false,
            vec!["재발급하지 못했습니다", "이전 토큰은 그대로"],
        ),
        (
            "recovered",
            T::Recovered,
            false,
            vec!["읽을 수 없어", "새 토큰", "환경변수"],
        ),
    ]
}

#[test]
fn token_panel_keeps_its_height_and_says_what_each_state_means() {
    for mode in [AppTheme::Midnight, AppTheme::Porcelain] {
        for width in [560., 900.] {
            let mut app = fixture();
            app.preferences.theme = mode;
            let mut renderer =
                iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
            let mut height = None;
            for (name, token, busy, expected) in token_panel_cases() {
                app.mcp_setup.token = token;
                app.mcp_setup.busy = busy;
                let mut element = crate::mcp_setup_ui::token_panel(&app.mcp_setup);
                let mut tree = Tree::new(element.as_widget());
                let node = element.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, Size::new(width, 2000.)),
                );
                let mut probe = SettingsProbe::default();
                element.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut probe,
                );
                let all = probe.texts.join(" | ");
                for text in expected {
                    assert!(all.contains(text), "{name}: missing {text:?} in {all}");
                }
                let measured = node.size().height;
                assert!(measured > 40., "{name}: the panel is missing");
                match height {
                    None => height = Some(measured),
                    Some(first) => assert_eq!(
                        measured, first,
                        "{name}: the panel changed height, moving every control below it"
                    ),
                }
                if let Some(output) = std::env::var_os("TIMESCHEDULE_MCP_SETUP_PREVIEW_DIR") {
                    let output = PathBuf::from(output);
                    std::fs::create_dir_all(&output).unwrap();
                    render_settings_frame(
                        &app,
                        &mut element,
                        &mut tree,
                        &node,
                        &mut renderer,
                        Size::new(width, measured),
                        &output.join(format!("token-{mode:?}-{width}-{name}.png")),
                    );
                }
            }
        }
    }
}

#[test]
fn reissuing_the_token_does_not_move_the_settings_controls() {
    for mode in [AppTheme::Midnight, AppTheme::Porcelain] {
        for size in [Size::new(800., 620.), Size::new(1160., 820.)] {
            let mut app = fixture();
            app.page = Page::Settings;
            app.settings_section = crate::app::SettingsSection::Mcp;
            // These panels sit in the advanced part, opened here.
            app.mcp_advanced = Some(true);
            app.preferences.theme = mode;
            let mut renderer =
                iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
            let mut tree = Tree::empty();
            let mut baseline = None;
            for (frame, (name, token, busy, _)) in token_panel_cases().into_iter().enumerate() {
                app.mcp_setup.token = token;
                app.mcp_setup.busy = busy;
                let mut element = app.view(app.manager);
                tree.diff(element.as_widget());
                let node = element.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, size),
                );
                if frame == 0 {
                    let mut set = SettingsProbe {
                        set_scroll: Some(280.),
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
                assert!(
                    probe.texts.iter().any(|text| text == "인증 토큰"),
                    "{name}: the settings page has no token panel"
                );
                let bounds: Vec<_> = probe.scrolls.iter().map(|s| s.0).collect();
                let actual = (probe.scrolls[1], bounds, probe.controls);
                if let Some(expected) = &baseline {
                    assert_eq!(&actual, expected, "{name}: the token panel moved controls");
                } else {
                    baseline = Some(actual);
                }
                if let Some(output) = std::env::var_os("TIMESCHEDULE_MCP_SETUP_PREVIEW_DIR") {
                    let output = PathBuf::from(output);
                    std::fs::create_dir_all(&output).unwrap();
                    render_settings_frame(
                        &app,
                        &mut element,
                        &mut tree,
                        &node,
                        &mut renderer,
                        size,
                        &output.join(format!("settings-token-{mode:?}-{}-{name}.png", size.width)),
                    );
                }
            }
        }
    }
}

#[test]
fn mcp_assistant_keeps_controls_and_outer_scroll_fixed_during_results() {
    for mode in [AppTheme::Midnight, AppTheme::Porcelain] {
        for size in [Size::new(800., 620.), Size::new(1160., 820.)] {
            let mut app = fixture();
            app.page = Page::Settings;
            app.settings_section = crate::app::SettingsSection::Mcp;
            // These panels sit in the advanced part, opened here.
            app.mcp_advanced = Some(true);
            app.mcp_setup.client = crate::mcp_setup::Client::ChatGptDesktop;
            app.preferences.theme = mode;
            let mut renderer =
                iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
            let mut tree = Tree::empty();
            let mut baseline = None;
            for (frame, status) in [
                "",
                "확인 중…",
                "서버 연결·인증 정상\n등록 설정 일치",
                "연결 실패\n긴 오류 설명\n원본 파일은 보존했습니다.\n다시 확인하세요.",
            ]
            .into_iter()
            .enumerate()
            {
                app.mcp_setup.busy = frame == 1;
                app.mcp_setup.status = status.into();
                let mut element = app.view(app.manager);
                tree.diff(element.as_widget());
                let node = element.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, size),
                );
                if frame == 0 {
                    let mut set = SettingsProbe {
                        set_scroll: Some(280.),
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
                let bounds: Vec<_> = probe.scrolls.iter().map(|s| s.0).collect();
                let actual = (probe.scrolls[1], bounds, probe.controls);
                if let Some(expected) = &baseline {
                    assert_eq!(&actual, expected, "MCP operation moved settings controls");
                } else {
                    baseline = Some(actual);
                }
                if let Some(output) = std::env::var_os("TIMESCHEDULE_MCP_SETUP_PREVIEW_DIR") {
                    let output = PathBuf::from(output);
                    std::fs::create_dir_all(&output).unwrap();
                    render_settings_frame(
                        &app,
                        &mut element,
                        &mut tree,
                        &node,
                        &mut renderer,
                        size,
                        &output.join(format!("mcp-{mode:?}-{}-{frame}.png", size.width)),
                    );
                }
            }
        }
    }
}

#[test]
fn settings_retained_layout_is_stable_across_native_busy_transitions() {
    // Keep the real scrollable tree alive across each change, as Iced does.
    // Removing the native controls while busy must fail this geometry check.
    for mode in [AppTheme::Midnight, AppTheme::Porcelain] {
        for size in [Size::new(620., 480.), Size::new(1160., 820.)] {
            let mut app = fixture();
            app.settings_section = crate::app::SettingsSection::Widget;
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
                        &probe.scrolls[probe.widget_status.unwrap()].0,
                        expected,
                        "status viewport must retain its width and position too"
                    );
                } else {
                    status_baseline = Some(probe.scrolls[probe.widget_status.unwrap()].0);
                }
                if let Some(expected) = &controls_baseline {
                    assert_eq!(&probe.controls, expected, "control targets changed");
                } else {
                    controls_baseline = Some(probe.controls);
                }

                assert_eq!(
                    probe.scrolls[probe.widget_status.unwrap()].0.height,
                    52.,
                    "status viewport keeps its height"
                );
                if frame >= 3 {
                    assert!(
                        probe.scrolls[probe.widget_status.unwrap()].1.height > 52.,
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
                    app.settings_section = crate::app::SettingsSection::Widget;
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
                        // The widget section holds no other scroll than its status.
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
                        let actual = (
                            probe.scrolls[1],
                            probe.scrolls[probe.widget_status.unwrap()].0,
                            probe.controls,
                        );
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
            app.settings_section = crate::app::SettingsSection::Widget;
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
fn widget_item_clicks_open_the_item_and_double_click_opens_the_day() {
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
        let title = calendar_text_bounds(app.view(app.widget.unwrap()), size)
            .into_iter()
            .find(|(s, _)| s == "완료할 일정")
            .expect("the item title is drawn in its cell")
            .1;
        let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
        let mut element = app.view(app.widget.unwrap());
        let mut tree = Tree::new(element.as_widget());
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
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
        let on_title = title.center();
        for _ in 0..2 {
            dispatch(click(true), on_title, &mut messages);
            dispatch(click(false), on_title, &mut messages);
        }
        assert!(
            busy || messages.iter().any(|m| matches!(
                m,
                Message::Calendar(crate::calendar_ui::Action::Open(_, _, true))
            )),
            "{messages:?}"
        );
        assert!(!messages.iter().any(|m| matches!(m, Message::WidgetDate(_))));
        messages.clear();
        let empty = Point::new(title.x + 30.0, title.y + 30.0);
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
    let quick_events = store
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
            let mut editor = Editor::new(Form::event(quick_events[0].clone()), 1);
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
                mouse::Cursor::Available(Point::new(310., 91.)),
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
            .any(|m| matches!(m, Message::Calendar(crate::calendar_ui::Action::Open(..)))),
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

#[test]
fn weekend_overrides_do_not_recolor_categories() {
    let mut app = calendar_record_fixture();
    let id = app.snapshot.categories[0].id.clone();
    let before = crate::calendar_ui::color(&app, &id);
    app.preferences.widget.dark.sunday = Some("#FFFF00".into());
    app.preferences.widget.dark.saturday = Some("#00FF00".into());
    assert_eq!(crate::calendar_ui::color(&app, &id), before);
}

#[test]
fn one_projects_records_share_a_line_and_open_as_cards() {
    use crate::calendar_ui::{message, open_message, Action};
    let mut app = calendar_record_fixture();
    let store = app.store.clone().unwrap();
    let project = store.list_projects().unwrap()[0].id.clone();
    let mut form = Form::new(Kind::Entry, app.selected.to_string(), Some(project.clone()));
    form.title = "둘째기록".into();
    form.body = (1..=9)
        .map(|n| format!("본문 {n}번째 줄"))
        .collect::<Vec<_>>()
        .join("\n");
    form.save(&store).unwrap();
    run_message(&mut app, Message::Load);

    let day = selected_day(&app).clone();
    let item = day
        .items
        .iter()
        .find(|i| i.project_id.as_ref() == Some(&project))
        .unwrap()
        .clone();
    assert_eq!((day.total, item.count), (1, 2), "two records, one line");
    let labels = calendar_texts(
        crate::calendar_ui::day_items(&app, Some(&day), 200., 60., 12., true),
        Size::new(200., 60.),
    );
    assert!(labels.iter().any(|s| s == "+ 2"), "{labels:?}");
    assert!(!labels.iter().any(|s| s.contains("기록")), "{labels:?}");

    run_message(&mut app, open_message(&item, false));
    assert!(app.calendar.detail.is_none());
    let group = app.calendar.group.as_ref().unwrap();
    assert_eq!(group.entries.len(), 2);
    let long = group
        .entries
        .iter()
        .find(|e| e.title == "둘째기록")
        .unwrap()
        .id
        .clone();
    let cards =
        |app: &App| calendar_texts(crate::calendar_ui::group_view(app), Size::new(520., 900.));
    let folded = cards(&app);
    for expected in ["TS", "연동기록", "둘째기록", "전체 보기", "수정", "닫기"] {
        assert!(
            folded.iter().any(|s| s == expected),
            "{expected}: {folded:?}"
        );
    }
    assert!(!folded.iter().any(|s| s.contains("9번째")), "{folded:?}");
    run_message(&mut app, message(Action::ToggleCard(long.clone())));
    assert!(cards(&app).iter().any(|s| s.contains("9번째")));

    // Editing a card keeps the cards underneath and refreshes them on save.
    run_message(&mut app, message(Action::EditCard(long.clone())));
    assert_eq!(app.editor.as_ref().unwrap().form.id.as_ref(), Some(&long));
    run_message(&mut app, Message::Field(Field::Title("고친기록".into())));
    run_message(&mut app, Message::Save);
    assert!(app.editor.is_none());
    let group = app.calendar.group.as_ref().unwrap();
    assert!(group.entries.iter().any(|e| e.title == "고친기록"));
    assert!(
        group.unfolded.contains(&long),
        "unfolded card stays unfolded"
    );

    // Deleting down to nothing closes the cards.
    for left in [1, 0] {
        let id = app.calendar.group.as_ref().unwrap().entries[0].id.clone();
        run_message(&mut app, message(Action::EditCard(id)));
        run_message(&mut app, Message::Delete);
        run_message(&mut app, Message::ConfirmDelete);
        assert_eq!(
            app.calendar.group.as_ref().map_or(0, |g| g.entries.len()),
            left
        );
    }
}

#[test]
fn delete_is_not_where_edit_and_save_are() {
    let mut app = calendar_record_fixture();
    let item = selected_day(&app).items[0].clone();
    run_message(&mut app, crate::calendar_ui::open_message(&item, false));
    run_message(
        &mut app,
        crate::calendar_ui::message(crate::calendar_ui::Action::EditDetail),
    );
    let labels = calendar_texts(crate::views::editor_view(&app), Size::new(520., 900.));
    let at = |s: &str| {
        labels
            .iter()
            .position(|l| l == s)
            .unwrap_or_else(|| panic!("{s}: {labels:?}"))
    };
    // Reading order: the delete control ends the form, before the footer.
    assert!(at("이 기록 삭제") < at("취소") && at("취소") < at("저장하기"));
    assert!(!labels.iter().any(|l| l == "삭제"));
}

#[test]
fn cards_belong_to_the_window_they_were_opened_in() {
    use crate::calendar_ui::{message, Action};
    let mut app = calendar_record_fixture();
    let store = app.store.clone().unwrap();
    let project = store.list_projects().unwrap()[0].id.clone();
    let mut form = Form::new(Kind::Entry, app.selected.to_string(), Some(project.clone()));
    form.title = "둘째기록".into();
    form.save(&store).unwrap();
    run_message(&mut app, Message::Load);
    let date = app.selected.to_string();
    let open = |quick| message(Action::OpenGroup(date.clone(), project.clone(), quick));

    // Diary cards survive the day window closing.
    run_message(&mut app, open(false));
    let quick = window::Id::unique();
    app.quick_window = Some(quick);
    app.quick_date = Some(app.selected);
    let _ = app.close_quick();
    assert!(app.calendar.group.as_ref().is_some_and(|g| !g.quick));

    // Day window cards go with it, and a card edit that was still loading
    // does not open an editor in a window that is gone.
    app.quick_window = Some(quick);
    app.quick_date = Some(app.selected);
    run_message(&mut app, open(true));
    let group = app.calendar.group.as_ref().unwrap();
    assert!(group.quick);
    let id = group.entries[0].id.clone();
    let loading = app.update(message(Action::EditCard(id)));
    let _ = app.update(Message::WindowClosed(quick));
    assert!(app.calendar.group.is_none());
    run_task(&mut app, loading);
    assert!(app.editor.is_none());
}

#[test]
fn automatic_record_card_and_detail_show_its_timeline_and_original() {
    use timeschedule_core::timeline::RecordCommit;
    let mut app = fixture();
    let store = app.store.clone().unwrap();
    let hash = "a".repeat(40);
    let mut record = crate::record_inbox::tests::sample();
    record.schema_version = 3;
    record.delivery_state = "local".into();
    record.space_url.clear();
    record.summary = "작업 배경과 결론".into();
    record.detail = "작업 배경과 결론\n\n긴 상세의 9번째 줄".into();
    record.status = "verified".into();
    record.started_at = Some("2026-10-31T00:05:00Z".into());
    record.completed_at = Some("2026-10-31T18:30:00+09:00".into());
    record.commits = vec![RecordCommit {
        hash: hash.clone(),
        committed_at: Some("2026-10-31T09:00:00Z".into()),
        subject: "시각 저장".into(),
        url: format!("https://github.com/o/r/commit/{hash}"),
    }];
    let id = store
        .apply_project_record(&record)
        .unwrap()
        .entry_id
        .unwrap();
    let entry = store.get_entry(&id).unwrap();
    app.snapshot = worker::snapshot(&store, &app.scope()).unwrap();
    let page = store
        .project_day_page(&entry.date, entry.project_id.as_ref().unwrap(), 0, 50)
        .unwrap();
    app.calendar.group = Some(crate::calendar_ui::Group {
        date: entry.date.clone(),
        project_id: entry.project_id.clone().unwrap(),
        quick: false,
        entries: page.entries,
        total: page.total,
        loading_more: false,
        timelines: store.entry_timelines(&[id.clone()]).unwrap(),
        unfolded: Default::default(),
    });
    // The user's reading order: title, start, the work, completion, then the
    // commits and the original. Checked on the real laid-out text.
    let order = [
        "원인과 검증 범위",
        "작업 시작 · 2026-10-31 09:05",
        "작업 배경과 결론",
        "완료 · 2026-10-31 18:30 · 확인한 범위 검증 완료",
        "커밋 · 2026-10-31 18:00 · aaaaaaa · 시각 저장",
        "커밋 열기",
        "원본 열기",
    ];
    fn in_order(labels: &[String], expected: &[&str]) {
        let at: Vec<usize> = expected
            .iter()
            .map(|s| {
                labels
                    .iter()
                    .position(|l| l.as_str() == *s)
                    .unwrap_or_else(|| panic!("{s}: {labels:?}"))
            })
            .collect();
        assert!(
            at.windows(2).all(|w| w[0] < w[1]),
            "{expected:?} in {labels:?}"
        );
    }
    let cards = calendar_texts(crate::calendar_ui::group_view(&app), Size::new(620., 900.));
    in_order(&cards, &order);
    assert!(cards.iter().any(|s| s == "전체 보기"), "{cards:?}");
    assert!(
        !cards.iter().any(|s| s.contains("9번째")),
        "folded to the summary"
    );
    assert_eq!(
        cards
            .iter()
            .filter(|s| s.as_str() == "작업 배경과 결론")
            .count(),
        1,
        "the summary is shown once"
    );
    run_message(
        &mut app,
        crate::calendar_ui::message(crate::calendar_ui::Action::ToggleCard(id.clone())),
    );
    let unfolded = calendar_texts(crate::calendar_ui::group_view(&app), Size::new(620., 900.));
    let body = unfolded.iter().position(|s| s.contains("9번째")).unwrap();
    let start = unfolded.iter().position(|s| s == order[1]).unwrap();
    let done = unfolded.iter().position(|s| s == order[3]).unwrap();
    assert!(start < body && body < done, "{unfolded:?}");

    // The single detail uses the same order.
    app.calendar.group = None;
    app.calendar.detail =
        Some(worker::hydrate_form(&store, Form::entry(store.get_entry(&id).unwrap())).unwrap());
    let detail = calendar_texts(crate::calendar_ui::detail_view(&app), Size::new(620., 900.));
    in_order(&detail, &[order[0], order[1], order[3], order[4], order[6]]);
    let body = detail.iter().position(|s| s.contains("9번째")).unwrap();
    assert!(
        detail.iter().position(|s| s == order[1]).unwrap() < body
            && body < detail.iter().position(|s| s == order[3]).unwrap(),
        "{detail:?}"
    );
    app.calendar.detail = None;

    // The project and records lists fold to the start and the chosen summary,
    // and unfold to the full text in the same order, without repeating it.
    let row = store
        .list_entries(&entry.date, &entry.date, None, 0)
        .unwrap()
        .into_iter()
        .find(|e| e.id == id)
        .unwrap();
    let folded = calendar_texts(crate::views::entry_card(&app, &row), Size::new(620., 900.));
    in_order(
        &folded,
        &["+ 원인과 검증 범위", order[1], "작업 배경과 결론"],
    );
    assert!(
        !folded
            .iter()
            .any(|s| s.contains("9번째") || s.starts_with("완료")),
        "{folded:?}"
    );
    run_message(&mut app, Message::ToggleEntry(id.clone()));
    let open = calendar_texts(crate::views::entry_card(&app, &row), Size::new(620., 900.));
    in_order(
        &open,
        &[
            "− 원인과 검증 범위",
            order[1],
            order[3],
            order[4],
            order[6],
            "기록 수정",
        ],
    );
    let body = open.iter().position(|s| s.contains("9번째")).unwrap();
    assert!(open.iter().position(|s| s == order[1]).unwrap() < body);
    assert!(body < open.iter().position(|s| s == order[3]).unwrap());
    assert!(
        !open.iter().any(|s| s == "작업 배경과 결론"),
        "summary not repeated: {open:?}"
    );
    assert!(
        !open.iter().any(|s| s.starts_with("space-work:")),
        "{open:?}"
    );

    // Pending work shows its status, never an invented completion time.
    record.revision = 2;
    record.status = "verification_pending".into();
    record.completed_at = None;
    store.apply_project_record(&record).unwrap();
    let pending =
        crate::calendar_ui::timeline_lines(&entry.date, &store.entry_timeline(&id).unwrap());
    assert!(
        pending.contains(&"현재 상태 · 검증 대기".to_string()),
        "{pending:?}"
    );
    assert!(!pending.iter().any(|l| l.starts_with("완료")));
}

#[test]
fn a_start_restored_on_opening_stays_after_folding_the_card_again() {
    let dir = crate::record_inbox::tests::TestDir::new();
    let state = dir.0.join("state");
    let receipt = state.join("diary-starts").join("r");
    std::fs::create_dir_all(&receipt).unwrap();
    let node = dir.0.join("node.exe");
    let script = dir.0.join("project-records.mjs");
    std::fs::write(&node, "").unwrap();
    std::fs::write(&script, "").unwrap();
    crate::record_sync::register(&dir.0, &node, &script, &state).unwrap();
    let mut app = fixture();
    app.data_dir = dir.0.clone();
    let store = app.store.clone().unwrap();
    let mut record = crate::record_inbox::tests::sample();
    record.work_id = uuid::Uuid::new_v4().to_string();
    let id = store
        .apply_project_record(&record)
        .unwrap()
        .entry_id
        .unwrap();
    std::fs::write(
        receipt.join("work.json"),
        format!(r#"{{"workId":"{}"}}"#, record.work_id),
    )
    .unwrap();
    std::fs::write(
        receipt.join("start.json"),
        r#"{"at":"2026-10-31T00:05:00Z","owner":"me@example.invalid"}"#,
    )
    .unwrap();
    // The list row was read before opening restored the start.
    let row = store
        .list_entries(&record.date, &record.date, None, 0)
        .unwrap()
        .into_iter()
        .find(|e| e.id == id)
        .unwrap();
    assert_eq!(row.timeline.started_at, None);
    let started = "작업 시작 · 2026-10-31 09:05";
    let card =
        |app: &App| calendar_texts(crate::views::entry_card(app, &row), Size::new(620., 900.));
    let before = card(&app);
    assert!(!before.iter().any(|s| s == started), "{before:?}");

    run_message(&mut app, Message::ToggleEntry(id.clone()));
    let open = card(&app);
    assert!(
        open.iter().any(|s| s == started),
        "restored on opening: {open:?}"
    );

    // Folded again, the card keeps the start it just showed, not unknown.
    run_message(&mut app, Message::ToggleEntry(id.clone()));
    assert!(!app.expanded.contains(&id));
    let folded = card(&app);
    assert!(folded.iter().any(|s| s == started), "{folded:?}");
    assert!(
        !folded
            .iter()
            .any(|s| s.starts_with("작업 시작") && s.ends_with("미상")),
        "{folded:?}"
    );
}

#[test]
fn hand_written_record_shows_its_typed_time_or_unknown() {
    let app = calendar_record_fixture();
    let store = app.store.clone().unwrap();
    let id = selected_day(&app).items[0].id.clone();
    let timeline = store.entry_timeline(&id).unwrap();
    let date = app.selected.to_string();
    assert_eq!(
        crate::calendar_ui::timeline_lines(&date, &timeline),
        vec!["기록 시각 · 미상".to_string()]
    );
    let mut form =
        worker::hydrate_form(&store, Form::entry(store.get_entry(&id).unwrap())).unwrap();
    form.time = "07:05".into();
    form.save(&store).unwrap();
    assert_eq!(
        crate::calendar_ui::timeline_lines(&date, &store.entry_timeline(&id).unwrap()),
        vec![format!("기록 시각 · {date} 07:05")]
    );
}

#[test]
fn project_page_groups_the_same_records_by_date_newest_first() {
    let mut app = calendar_record_fixture();
    let store = app.store.clone().unwrap();
    let project = store.list_projects().unwrap()[0].id.clone();
    let earlier = (app.selected - Duration::days(1)).to_string();
    let today = app.selected.to_string();
    for (date, title) in [(&earlier, "전날기록"), (&today, "같은날")] {
        let mut form = Form::new(Kind::Entry, date.clone(), Some(project.clone()));
        form.title = title.into();
        form.save(&store).unwrap();
    }
    run_message(&mut app, Message::Project(Some(project.clone())));
    let groups = crate::views::date_groups(&app.snapshot.entries);
    let shape: Vec<_> = groups
        .iter()
        .map(|(d, e)| (d.to_string(), e.len()))
        .collect();
    assert_eq!(
        shape,
        vec![(app.selected.to_string(), 2), (earlier.clone(), 1)],
        "newest date first, each count is its cards"
    );
    let labels = calendar_texts(crate::views::projects(&app), Size::new(900., 1400.));
    let year = chrono::Datelike::year(&chrono::Local::now());
    let day = |date: &str| crate::views::date_heading(date, year);
    let heading = format!("{} · 2건", day(&app.selected.to_string()));
    let at = |s: &str| {
        labels
            .iter()
            .position(|l| l == s)
            .unwrap_or_else(|| panic!("{s}: {labels:?}"))
    };
    assert!(at(&heading) < at(&format!("{} · 1건", day(&earlier))));
}

#[test]
fn a_day_of_51_records_counts_all_and_reads_the_rest_a_step_later() {
    use crate::calendar_ui::{message, open_message, Action};
    use std::collections::HashSet;
    let mut app = calendar_record_fixture();
    let store = app.store.clone().unwrap();
    let project = store.list_projects().unwrap()[0].id.clone();
    // The fixture's record and 50 more: one past the cards' 50-record step.
    for n in 1..=50 {
        let mut form = Form::new(Kind::Entry, app.selected.to_string(), Some(project.clone()));
        form.title = format!("묶음 {n}");
        form.save(&store).unwrap();
    }
    run_message(&mut app, Message::Load);
    let item = selected_day(&app)
        .items
        .iter()
        .find(|i| i.project_id.as_ref() == Some(&project))
        .unwrap()
        .clone();
    assert_eq!(item.count, 51);
    run_message(&mut app, open_message(&item, false));
    let group = app.calendar.group.as_ref().unwrap();
    assert_eq!((group.entries.len(), group.total), (50, 51));
    let date = app.selected.to_string();
    let labels = calendar_texts(crate::calendar_ui::group_view(&app), Size::new(620., 900.));
    assert!(
        labels.iter().any(|s| *s == format!("{date} · 기록 51개")),
        "{labels:?}"
    );
    assert!(
        labels.iter().any(|s| s == "기록 더 보기 · 50/51개"),
        "{labels:?}"
    );

    run_message(&mut app, message(Action::GroupMore));
    let group = app.calendar.group.as_ref().unwrap();
    let ids: HashSet<_> = group.entries.iter().map(|e| e.id.clone()).collect();
    assert_eq!((group.entries.len(), ids.len(), group.total), (51, 51, 51));
    assert!(group
        .entries
        .iter()
        .all(|e| group.timelines.contains_key(&e.id)));
    let labels = calendar_texts(crate::calendar_ui::group_view(&app), Size::new(620., 900.));
    assert!(
        !labels.iter().any(|s| s.starts_with("기록 더 보기")),
        "{labels:?}"
    );
    // A refresh brings back every card already read, each once.
    run_message(&mut app, Message::Load);
    let group = app.calendar.group.as_ref().unwrap();
    let again: HashSet<_> = group.entries.iter().map(|e| e.id.clone()).collect();
    assert_eq!((group.entries.len(), again), (51, ids));
}

#[test]
fn project_heading_counts_every_record_of_a_date_past_the_first_200() {
    let mut app = calendar_record_fixture();
    let store = app.store.clone().unwrap();
    let project = store.list_projects().unwrap()[0].id.clone();
    let today = app.selected.to_string();
    let heading = crate::views::date_heading(&today, chrono::Datelike::year(&chrono::Local::now()));
    // The fixture's record and 200 more on one date.
    for n in 1..=200 {
        let mut form = Form::new(Kind::Entry, today.clone(), Some(project.clone()));
        form.title = format!("하루 {n}");
        form.save(&store).unwrap();
    }
    run_message(&mut app, Message::Project(Some(project.clone())));
    assert_eq!(app.snapshot.entries.len(), 200);
    assert!(app.more);
    let labels = calendar_texts(crate::views::projects(&app), Size::new(900., 1400.));
    let partial = format!("{heading} · 201건 · 200건 표시 · 더 불러오면 이어집니다");
    assert!(
        labels.iter().any(|s| *s == partial),
        "{:?}",
        &labels[..8.min(labels.len())]
    );
    run_message(&mut app, Message::More);
    let ids: std::collections::HashSet<_> =
        app.snapshot.entries.iter().map(|e| e.id.clone()).collect();
    assert_eq!((app.snapshot.entries.len(), ids.len()), (201, 201));
    let labels = calendar_texts(crate::views::projects(&app), Size::new(900., 1400.));
    assert!(labels.iter().any(|s| *s == format!("{heading} · 201건")));
}

#[test]
fn a_project_line_stays_on_one_line_in_a_narrow_cell_of_any_height() {
    let app = calendar_record_fixture();
    let mut day = selected_day(&app).clone();
    day.items[0].project_short_name = "아주 긴 프로젝트 이름입니다".into();
    day.items[0].count = 12;
    for height in [20., 60., 400.] {
        for quick in [false, true] {
            let labels = calendar_text_bounds(
                crate::calendar_ui::day_items(&app, Some(&day), 90., height, 12., quick),
                Size::new(90., height),
            );
            let count = labels
                .iter()
                .find(|(s, _)| s == "+ 12")
                .unwrap_or_else(|| panic!("{labels:?}"));
            // Only the name and its count: no second line of the name.
            assert_eq!(labels.len(), 2, "{height} {quick}: {labels:?}");
            let name = labels.iter().find(|(s, _)| s != "+ 12").unwrap();
            assert!(name.0.ends_with('…'), "{labels:?}");
            assert!((name.1.y - count.1.y).abs() < 0.5, "same line: {labels:?}");
        }
    }
}

#[test]
fn only_a_hand_written_record_offers_a_typed_time() {
    let mut app = calendar_record_fixture();
    let store = app.store.clone().unwrap();
    let id = selected_day(&app).items[0].id.clone();
    let label = "기록 시각 (한국시간 24시간제 HH:mm, 비우면 미상)";
    let fields = |app: &App| calendar_texts(crate::views::editor_view(app), Size::new(620., 1400.));
    let form = worker::hydrate_form(&store, Form::entry(store.get_entry(&id).unwrap())).unwrap();
    let _ = app.open_form(form);
    assert!(fields(&app).iter().any(|s| s == label));
    app.editor = None;
    // An automatic record keeps its begin receipt; no typed time is offered.
    let mut record = crate::record_inbox::tests::sample();
    record.date = app.selected.to_string();
    let auto = store
        .apply_project_record(&record)
        .unwrap()
        .entry_id
        .unwrap();
    let mut form =
        worker::hydrate_form(&store, Form::entry(store.get_entry(&auto).unwrap())).unwrap();
    assert!(!form.takes_manual_time());
    let _ = app.open_form(form.clone());
    assert!(!fields(&app).iter().any(|s| s == label));
    app.editor = None;
    // Even a typed value sent anyway is not stored for it.
    form.time = "07:00".into();
    form.title = "고친 제목".into();
    form.save(&store).unwrap();
    let t = store.entry_timeline(&auto).unwrap();
    assert_eq!(t.manual_time, None);
    assert!(crate::calendar_ui::timeline_lines(&record.date, &t)[0].starts_with("작업 시작 · "));
}

#[test]
fn project_chat_review_requires_checked_candidates_and_preserves_them_on_reread() {
    use crate::chat_review_ui::Action;
    use serde_json::json;
    use timeschedule_core::{chat_experiences::Kind as ExperienceKind, diary::ProjectInput};
    let dir = crate::record_inbox::tests::TestDir::new();
    let mut app = fixture();
    let store = app.store.clone().unwrap();
    let project = store
        .save_project(ProjectInput {
            id: None,
            title: "검토 대상".into(),
            description: String::new(),
            repo_path: dir.0.to_string_lossy().into_owned(),
        })
        .unwrap()
        .id;
    store.set_chat_capture(&project, true).unwrap();
    let session = uuid::Uuid::new_v4().to_string();
    let folder = dir.0.join("sessions/2026/11/19");
    std::fs::create_dir_all(&folder).unwrap();
    let mut lines=vec![json!({"type":"session_meta","payload":{"id":session,"cwd":dir.0,"originator":"Codex Desktop","timestamp":"2026-11-19T10:00:00Z"}}).to_string()];
    for (id, role, body) in [
        ("request", "UserMessage", "설계 이유를 설명해줘."),
        (
            "answer",
            "AgentMessage",
            "환경이 다르기 때문에 같은 설정을 공유합니다.",
        ),
    ] {
        lines.push(json!({"type":"event_msg","payload":{"type":"item_completed","thread_id":session,"item":{"type":role,"id":id,"content":[{"text":body}]}}}).to_string());
    }
    std::fs::write(
        folder.join(format!("rollout-example-{session}.jsonl")),
        lines.join("\n") + "\n",
    )
    .unwrap();
    assert_eq!(store.scan_codex_chats(&dir.0, true).unwrap().saved, 1);
    run_message(&mut app, Message::Project(Some(project.clone())));
    let choice = app.snapshot.reviewable_chats[0].clone();
    for _ in 0..2 {
        run_message(
            &mut app,
            Message::ChatReview(Action::Read(
                project.clone(),
                store.review_chat_with_inference(&project, &choice.id, |_| {
                    Ok(json!({
                        "design": {"title": "설정 공유", "problem": 1, "reason": 2,
                                   "solution": 2, "test": 0, "limit": 0},
                        "troubleshooting": null
                    })
                    .to_string())
                }),
            )),
        );
        let labels = calendar_texts(
            crate::chat_review_ui::panel(&app, &project),
            Size::new(800., 800.),
        );
        assert!(labels.iter().any(|l| l == "확인한 후보만 이력에 저장"));
        let before = store.portfolio_panel(&project).unwrap().experiences;
        run_message(&mut app, Message::ChatReview(Action::Save));
        assert_eq!(store.portfolio_panel(&project).unwrap().experiences, before);
        run_message(
            &mut app,
            Message::ChatReview(Action::Check(ExperienceKind::Design, true)),
        );
        run_message(&mut app, Message::ChatReview(Action::Save));
        assert_eq!(store.portfolio_panel(&project).unwrap().experiences, 1);
    }
}

#[test]
#[ignore = "manual previews of the past-days area and the project editor's folder row"]
fn render_past_days_previews() {
    use timeschedule_core::diary::ProjectInput;
    let output = PathBuf::from(std::env::var_os("TIMESCHEDULE_PREVIEW_DIR").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let dir = crate::record_inbox::tests::TestDir::new();
    let mut app = fixture();
    let store = app.store.clone().unwrap();
    let project = store
        .save_project(ProjectInput {
            id: None,
            title: "midi".into(),
            description: "MIDI 편집 프로그램".into(),
            repo_path: dir.0.to_string_lossy().into_owned(),
        })
        .unwrap()
        .id;
    // The capture is on for an empty folder: no session is read.
    store.set_chat_capture(&project, true).unwrap();
    run_message(&mut app, Message::Project(Some(project.clone())));
    // The past-days panel sits in the review tab.
    let _ = app.update(Message::ProjectTab(crate::app::ProjectTab::Review));
    let days: Vec<NaiveDate> = [20, 22, 23, 24]
        .iter()
        .map(|d| NaiveDate::from_ymd_opt(2026, 11, *d).unwrap())
        .collect();
    let size = Size::new(1160., 820.);
    let frame = |app: &App, name: &str| {
        let mut element = crate::views::view(app, app.manager);
        let mut renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
        let mut tree = Tree::new(element.as_widget());
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        render_scaled_frame(
            app,
            &mut element,
            &mut tree,
            &node,
            &mut renderer,
            size,
            &output.join(format!("{name}.png")),
            1.,
        );
    };
    app.manager_size = size;
    for (name, state, making) in [
        ("past-1-button", None, false),
        ("past-2-checking", Some(Past::Checking), true),
        (
            "past-3-ask",
            Some(Past::Ask(days.clone(), "root".into())),
            false,
        ),
        ("past-4-making", Some(Past::Making), true),
        ("past-5-other-job", None, true),
    ] {
        app.past_days.clear();
        app.past_days.extend(state.map(|s| (project.clone(), s)));
        app.daily_making = making;
        frame(&app, name);
    }
    app.past_days.clear();
    app.daily_making = false;
    let _ = app.update(Message::New(Kind::Project, None));
    frame(&app, "editor-1-empty");
    let generation = app.editor.as_ref().unwrap().generation;
    let _ = app.update(Message::RepoFolderPicked(generation, None));
    app.editor.as_mut().unwrap().picking_repo = true;
    let _ = app.update(Message::RepoFolderPicked(
        generation,
        Some(PathBuf::from(
            r"C:\LSH\PROJECTS\some-rather-long-folder-name\nested\repository-with-a-long-name",
        )),
    ));
    frame(&app, "editor-2-picked");
}

#[test]
fn a_project_date_heading_names_the_weekday_and_drops_this_year() {
    use crate::views::date_heading;
    assert_eq!(date_heading("2026-11-24", 2026), "11월 24일 (화)");
    assert_eq!(date_heading("2025-11-24", 2026), "2025년 11월 24일 (월)");
    assert_eq!(date_heading("날짜 없음", 2026), "날짜 없음");
}

#[test]
fn a_busy_project_keeps_its_draft_buttons_in_place() {
    let mut app = calendar_record_fixture();
    let project = app.snapshot.projects[0].id.clone();
    run_message(&mut app, Message::Project(Some(project)));
    let size = Size::new(900., 1400.);
    // 근거 요약 sits in the records tab, 자료 발췌 초안 in the portfolio tab.
    let press = |app: &mut App, label: &str| {
        app.project_tab = if label == "근거 요약" {
            crate::app::ProjectTab::Records
        } else {
            crate::app::ProjectTab::Portfolio
        };
        let target = calendar_text_bounds(crate::views::projects(app), size)
            .into_iter()
            .find(|(s, _)| s == label)
            .unwrap_or_else(|| panic!("{label} is on screen"))
            .1;
        click_text(crate::views::projects(app), size, target)
    };
    assert!(matches!(
        press(&mut app, "근거 요약").as_slice(),
        [Message::Draft(false)]
    ));
    assert!(matches!(
        press(&mut app, "자료 발췌 초안").as_slice(),
        [Message::Draft(true)]
    ));
    app.project_tab = crate::app::ProjectTab::Records;
    app.busy = true;
    let labels = calendar_texts(crate::views::projects(&app), size);
    assert!(labels.iter().any(|s| s == "작업 중…"), "{labels:?}");
    for label in ["근거 요약", "자료 발췌 초안"] {
        assert!(
            press(&mut app, label).is_empty(),
            "{label} is pressable while busy"
        );
    }
    // A draft finished during a refresh cannot open its editor, so a refresh blocks too.
    app.busy = false;
    app.loading = true;
    for label in ["근거 요약", "자료 발췌 초안"] {
        assert!(
            press(&mut app, label).is_empty(),
            "{label} is pressable while loading"
        );
    }
}

#[test]
fn a_project_says_what_is_missing_instead_of_hiding_it() {
    let mut app = fixture();
    let store = app.store.clone().unwrap();
    let project = store
        .save_project(timeschedule_core::diary::ProjectInput {
            id: None,
            title: "빈 프로젝트".into(),
            description: String::new(),
            repo_path: String::new(),
        })
        .unwrap();
    run_message(&mut app, Message::Project(Some(project.id)));
    let labels = calendar_texts(crate::views::projects(&app), Size::new(900., 1400.));
    for label in [crate::views::NO_REPO_HINT, crate::views::NO_PROJECT_RECORDS] {
        assert!(labels.iter().any(|s| s == label), "{label}: {labels:?}");
    }
    // No empty line stands in for the missing description.
    assert!(!labels.iter().any(|s| s.is_empty()), "{labels:?}");
}

#[test]
fn every_project_has_a_shortcut_and_today_shows_only_on_the_calendars() {
    let mut app = fixture();
    let store = app.store.clone().unwrap();
    for n in 1..=8 {
        store
            .save_project(timeschedule_core::diary::ProjectInput {
                id: None,
                title: format!("바로가기 {n}"),
                description: String::new(),
                repo_path: String::new(),
            })
            .unwrap();
    }
    run_message(&mut app, Message::Load);
    let labels = calendar_texts(crate::shell::view(&app), Size::new(1160., 1600.));
    for n in 1..=8 {
        let name = format!("바로가기 {n}");
        assert!(labels.iter().any(|s| *s == name), "{name}: {labels:?}");
    }
    for page in [
        Page::Month,
        Page::Week,
        Page::Projects,
        Page::Records,
        Page::Tasks,
        Page::Notes,
        Page::Settings,
    ] {
        run_message(&mut app, Message::Navigate(page));
        let labels = calendar_texts(crate::shell::view(&app), Size::new(1160., 1600.));
        let calendar = matches!(page, Page::Month | Page::Week);
        assert_eq!(
            labels.iter().any(|s| s == "오늘"),
            calendar,
            "{page:?}: {labels:?}"
        );
    }
}

#[test]
fn a_failed_load_never_calls_the_next_project_empty() {
    let mut app = fixture();
    let store = app.store.clone().unwrap();
    let input = |title: &str| timeschedule_core::diary::ProjectInput {
        id: None,
        title: title.into(),
        description: String::new(),
        repo_path: String::new(),
    };
    let empty = store.save_project(input("빈 프로젝트")).unwrap();
    let full = store.save_project(input("기록 있는 프로젝트")).unwrap();
    let mut form = Form::new(Kind::Entry, app.selected.to_string(), Some(full.id.clone()));
    form.title = "남긴 기록".into();
    form.save(&store).unwrap();
    let says_empty = |app: &App| {
        calendar_texts(crate::views::projects(app), Size::new(900., 1400.))
            .iter()
            .any(|s| s == crate::views::NO_PROJECT_RECORDS)
    };
    run_message(&mut app, Message::Project(Some(empty.id.clone())));
    assert!(says_empty(&app));
    // The next project's load fails; the empty rows on screen are still the first one's.
    let _ = app.update(Message::Project(Some(full.id.clone())));
    let _ = app.update(Message::Loaded(
        app.view_generation,
        Err("읽기 실패".into()),
    ));
    app.error.clear();
    assert!(!says_empty(&app));
    // The first record is saved, then the reload fails: the old empty rows prove nothing.
    run_message(&mut app, Message::Project(Some(empty.id.clone())));
    assert!(says_empty(&app));
    let mut first = Form::new(
        Kind::Entry,
        app.selected.to_string(),
        Some(empty.id.clone()),
    );
    first.title = "첫 기록".into();
    first.save(&store).unwrap();
    let _ = app.update(Message::Load);
    let _ = app.update(Message::Loaded(
        app.view_generation,
        Err("읽기 실패".into()),
    ));
    assert!(!says_empty(&app));
    // An empty project loaded after an error says so, whatever the error line holds.
    let other = store.save_project(input("또 빈 프로젝트")).unwrap();
    app.error = "이전 오류".into();
    run_message(&mut app, Message::Project(Some(other.id)));
    assert!(says_empty(&app));
}

#[test]
fn settings_shows_the_section_its_button_picks() {
    use crate::app::SettingsSection;
    let mut app = fixture();
    run_message(&mut app, Message::Navigate(Page::Settings));
    let size = Size::new(1160., 1600.);
    let shell_texts = |app: &App| calendar_texts(crate::shell::view(app), size);
    let labels = shell_texts(&app);
    for section in SettingsSection::ALL {
        assert!(
            labels.iter().any(|s| s == section.label()),
            "{section:?}: {labels:?}"
        );
    }
    assert!(labels.iter().any(|s| s == "화면 모드"), "{labels:?}");
    assert!(!labels.iter().any(|s| s == "로컬 일정 MCP"), "{labels:?}");
    // Pressing a section button sends that section and shows only it.
    let target = calendar_text_bounds(crate::shell::view(&app), size)
        .into_iter()
        .find(|(s, _)| s == "AI 연결 (MCP)")
        .unwrap()
        .1;
    let messages = click_text(crate::shell::view(&app), size, target);
    assert!(
        matches!(
            messages.as_slice(),
            [Message::SettingsSection(SettingsSection::Mcp)]
        ),
        "{messages:?}"
    );
    run_message(&mut app, Message::SettingsSection(SettingsSection::Mcp));
    let labels = shell_texts(&app);
    assert!(labels.iter().any(|s| s == "로컬 일정 MCP"), "{labels:?}");
    assert!(!labels.iter().any(|s| s == "화면 모드"), "{labels:?}");
    // Leaving and coming back keeps the section.
    run_message(&mut app, Message::Navigate(Page::Month));
    run_message(&mut app, Message::Navigate(Page::Settings));
    assert_eq!(app.settings_section, SettingsSection::Mcp);
}

#[test]
fn the_widget_settings_button_opens_the_widget_section_with_opacity_by_the_colors() {
    use crate::app::SettingsSection;
    let mut app = fixture();
    app.settings_section = SettingsSection::Mcp;
    let _ = app.update(Message::WidgetSetting(crate::widget_settings::Change::Open));
    assert_eq!(
        (app.page, app.settings_section),
        (Page::Settings, SettingsSection::Widget)
    );
    let labels = calendar_texts(crate::views::settings(&app), Size::new(1000., 2400.));
    let at = |label: &str| {
        labels
            .iter()
            .position(|s| s.starts_with(label))
            .unwrap_or_else(|| panic!("{label}: {labels:?}"))
    };
    assert!(at("표시와 위치") < at("크기와 표시 기간"));
    assert!(at("크기와 표시 기간") < at("글꼴과 간격"));
    assert!(at("글꼴과 간격") < at("색상과 불투명도"));
    assert!(at("색상과 불투명도") < at("배경 불투명도"));
    // The slider right under the opacity line sets the opacity.
    let size = Size::new(1000., 2400.);
    let line = calendar_text_bounds(crate::views::settings(&app), size)
        .into_iter()
        .find(|(s, _)| s.starts_with("배경 불투명도"))
        .unwrap()
        .1;
    // The column puts 14px between the line and the 22px slider.
    let slider = Rectangle::new(
        Point::new(line.x + 40., line.y + line.height + 20.),
        Size::new(4., 10.),
    );
    let messages = click_text(crate::views::settings(&app), size, slider);
    assert!(
        matches!(
            messages.as_slice(),
            [Message::Native(NativeAction::Opacity(_)), ..]
        ),
        "{messages:?}"
    );
}

#[test]
fn widget_errors_stay_in_the_banner_outside_the_widget_section() {
    use crate::app::SettingsSection;
    let mut app = fixture();
    let _ = app.update(Message::NativeDone(
        NativeAction::Recover,
        Err("위젯 복구 실패 fixture".into()),
    ));
    let size = Size::new(1160., 1600.);
    let banner = |app: &App| {
        calendar_texts(crate::shell::view(app), size)
            .iter()
            .filter(|s| s.contains("위젯 복구 실패 fixture"))
            .count()
    };
    app.page = Page::Settings;
    for section in [
        SettingsSection::General,
        SettingsSection::Records,
        SettingsSection::Mcp,
    ] {
        app.settings_section = section;
        assert_eq!(banner(&app), 1, "{section:?} shows the error once");
    }
    // The widget section shows it in its own status, not twice.
    app.settings_section = SettingsSection::Widget;
    assert_eq!(banner(&app), 1);
}

/// The widget operations a task asks for, such as scrolling, without running it.
fn widget_operations(
    task: Task<Message>,
) -> Vec<Box<dyn iced::advanced::widget::Operation + Send>> {
    use iced::futures::StreamExt;
    let mut operations = Vec::new();
    let Some(mut stream) = iced_runtime::task::into_stream(task) else {
        return operations;
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        // Window and file work may never finish here; only the requests made at once count.
        let _ = tokio::time::timeout(std::time::Duration::from_millis(300), async {
            while let Some(action) = stream.next().await {
                if let iced_runtime::Action::Widget(operation) = action {
                    operations.push(operation);
                }
            }
        })
        .await;
    });
    operations
}

#[test]
fn every_way_into_a_settings_section_starts_at_its_top() {
    use crate::app::SettingsSection;
    let size = Size::new(1160., 820.);
    let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
    let settings_offset = |app: &App, tree: &mut Tree, scroll_to: Option<f32>, ops| {
        let mut element = crate::views::settings(app);
        tree.diff(element.as_widget());
        let node =
            element
                .as_widget_mut()
                .layout(tree, &renderer, &layout::Limits::new(Size::ZERO, size));
        let layout = layout::Layout::new(&node);
        let mut set = SettingsProbe {
            set_scroll: scroll_to,
            set_scroll_index: 0,
            ..Default::default()
        };
        element
            .as_widget_mut()
            .operate(tree, layout, &renderer, &mut set);
        for mut operation in ops {
            let operation: &mut Box<dyn iced::advanced::widget::Operation + Send> = &mut operation;
            element
                .as_widget_mut()
                .operate(tree, layout, &renderer, operation.as_mut());
        }
        let mut probe = SettingsProbe::default();
        element
            .as_widget_mut()
            .operate(tree, layout, &renderer, &mut probe);
        probe.scrolls[0].2.y
    };
    for (name, message) in [
        (
            "section button",
            Message::SettingsSection(SettingsSection::Widget),
        ),
        (
            "widget settings button",
            Message::WidgetSetting(crate::widget_settings::Change::Open),
        ),
    ] {
        let mut app = fixture();
        app.page = Page::Settings;
        app.settings_section = SettingsSection::Widget;
        let mut tree = Tree::empty();
        let scrolled = settings_offset(&app, &mut tree, Some(300.), Vec::new());
        assert!(scrolled > 0., "{name}: the widget section scrolls");
        let task = app.update(message);
        let offset = settings_offset(&app, &mut tree, None, widget_operations(task));
        assert_eq!(offset, 0., "{name}");
    }
    // Moving to another section from a scrolled one starts that section at its top too.
    let mut app = fixture();
    app.page = Page::Settings;
    app.settings_section = SettingsSection::Widget;
    let mut tree = Tree::empty();
    assert!(settings_offset(&app, &mut tree, Some(300.), Vec::new()) > 0.);
    let task = app.update(Message::SettingsSection(SettingsSection::Mcp));
    let operations = widget_operations(task);
    assert_eq!(settings_offset(&app, &mut tree, None, operations), 0.);
}

#[test]
fn opening_a_settings_section_ends_a_search_and_rereads_the_ai_apps() {
    use crate::app::SettingsSection;
    let mut app = fixture();
    let _ = app.update(Message::WidgetSetting(crate::widget_settings::Change::Open));
    app.search = "회의".into();
    let generation = app.mcp_setup.generation;
    let _ = app.update(Message::SettingsSection(SettingsSection::Mcp));
    assert!(
        app.search.is_empty(),
        "the section is not hidden behind results"
    );
    assert_eq!(
        app.mcp_setup.generation,
        generation + 1,
        "registrations are read again"
    );
    app.search = "회의".into();
    let _ = app.update(Message::WidgetSetting(crate::widget_settings::Change::Open));
    assert_eq!(
        (app.search.as_str(), app.settings_section),
        ("", SettingsSection::Widget)
    );
}

#[test]
fn an_editor_still_loading_does_not_open_over_a_new_settings_section() {
    use crate::app::SettingsSection;
    let mut app = fixture();
    app.page = Page::Settings;
    let (vg, eg) = (app.view_generation, app.editor_generation);
    let form = Form::new(Kind::Note, app.selected.to_string(), None);
    let _ = app.update(Message::SettingsSection(SettingsSection::Mcp));
    let _ = app.update(Message::FormLoaded(vg, eg, Ok(form)));
    assert!(app.editor.is_none(), "the late search result stays closed");
}

#[test]
fn the_token_and_helper_fold_away_until_needed_or_opened() {
    use crate::app::SettingsSection;
    use crate::mcp_clients::Registration;
    use crate::mcp_setup_ui::TokenPanel;
    let mut app = fixture();
    app.page = Page::Settings;
    app.settings_section = SettingsSection::Mcp;
    let size = Size::new(1160., 2400.);
    let shows = |app: &App, label: &str| {
        calendar_texts(crate::views::settings(app), size)
            .iter()
            .any(|s| s == label)
    };
    // Nothing needs attention: the advanced part stays folded.
    assert!(!shows(&app, "인증 토큰") && !shows(&app, "AI 연결 도우미"));
    let target = calendar_text_bounds(crate::views::settings(&app), size)
        .into_iter()
        .find(|(s, _)| s == "펼치기")
        .unwrap()
        .1;
    let messages = click_text(crate::views::settings(&app), size, target);
    assert!(matches!(messages.as_slice(), [Message::McpAdvanced(true)]));
    let _ = app.update(Message::McpAdvanced(true));
    assert!(shows(&app, "인증 토큰") && shows(&app, "AI 연결 도우미") && shows(&app, "접기"));
    // It opens by itself for a replaced token, or when no AI app is registered.
    for needs in [0, 1] {
        let mut app = fixture();
        app.settings_section = SettingsSection::Mcp;
        if needs == 0 {
            app.mcp_setup.token = TokenPanel::Recovered;
        } else {
            app.mcp_setup.registrations = std::array::from_fn(|_| Registration::Missing);
        }
        assert!(shows(&app, "인증 토큰"), "case {needs}");
        // The owner's choice to close it wins.
        let _ = app.update(Message::McpAdvanced(false));
        assert!(!shows(&app, "인증 토큰"), "case {needs}");
    }
    // Opened by itself for a replaced token, it stays open through the reissue steps.
    let mut app = fixture();
    app.settings_section = SettingsSection::Mcp;
    app.mcp_setup.token = TokenPanel::Recovered;
    let _ = app.update(Message::McpSetup(crate::mcp_setup_ui::Action::ReissueAsk));
    assert_eq!(app.mcp_setup.token, TokenPanel::Confirming);
    assert!(
        shows(&app, "재발급 확인"),
        "the confirm button stays in reach"
    );
    // Opened by itself because no AI app is registered, it stays open while the board
    // is read again and while the owner moves on to the next app.
    let mut app = fixture();
    app.settings_section = SettingsSection::Mcp;
    let generation = app.mcp_setup.generation;
    let _ = app.update(Message::McpSetup(crate::mcp_setup_ui::Action::Loaded(
        generation,
        std::array::from_fn(|_| Registration::Missing),
    )));
    assert_eq!(app.mcp_advanced, Some(true));
    app.mcp_setup.checking = true;
    assert!(
        shows(&app, "AI 연결 도우미"),
        "a new reading does not fold it"
    );
    app.mcp_setup.checking = false;
    app.mcp_setup.registrations[0] = Registration::Current;
    let _ = app.update(Message::McpSetup(crate::mcp_setup_ui::Action::Client(
        crate::mcp_setup::Client::ClaudeCode,
    )));
    assert!(
        shows(&app, "AI 연결 도우미"),
        "choosing the next app keeps it open"
    );
    // A board row's button shows its result in the helper, so the helper opens.
    let mut app = fixture();
    app.mcp_advanced = Some(false);
    let _ = app.update(Message::McpSetup(crate::mcp_setup_ui::Action::Inspect(
        crate::mcp_setup::Client::Codex,
    )));
    assert_eq!(app.mcp_advanced, Some(true));
}

#[test]
fn the_screen_mode_buttons_switch_both_ways_and_ignore_the_mode_in_use() {
    let mut app = fixture();
    app.page = Page::Settings;
    app.preferences.theme = AppTheme::Midnight;
    let size = Size::new(1160., 1600.);
    let target = calendar_text_bounds(crate::views::settings(&app), size)
        .into_iter()
        .find(|(s, _)| s == "밝은 모드")
        .unwrap()
        .1;
    let messages = click_text(crate::views::settings(&app), size, target);
    assert!(
        matches!(
            messages.as_slice(),
            [Message::SetTheme(AppTheme::Porcelain)]
        ),
        "{messages:?}"
    );
    let _ = app.update(Message::SetTheme(AppTheme::Porcelain));
    assert_eq!(app.preferences.theme, AppTheme::Porcelain);
    // Choosing the mode in use changes nothing.
    let _ = app.update(Message::SetTheme(AppTheme::Porcelain));
    assert_eq!(app.preferences.theme, AppTheme::Porcelain);
    // From the light mode, the other button goes back to the dark one.
    let target = calendar_text_bounds(crate::views::settings(&app), size)
        .into_iter()
        .find(|(s, _)| s == "어두운 모드")
        .unwrap()
        .1;
    let messages = click_text(crate::views::settings(&app), size, target);
    assert!(
        matches!(messages.as_slice(), [Message::SetTheme(AppTheme::Midnight)]),
        "{messages:?}"
    );
    let _ = app.update(Message::SetTheme(AppTheme::Midnight));
    assert_eq!(app.preferences.theme, AppTheme::Midnight);
}

#[test]
fn the_project_picker_replaces_the_list_inside_the_screen() {
    let mut app = calendar_record_fixture();
    run_message(&mut app, Message::Navigate(Page::Projects));
    let size = Size::new(1160., 1600.);
    // The picker offers all projects first, then each project, and shows the open one.
    let project = app.snapshot.projects[0].clone();
    let (choices, selected) = crate::views::project_choices(&app);
    let offered: Vec<_> = choices
        .iter()
        .map(|c| (c.id.clone(), c.title.as_str()))
        .collect();
    assert_eq!(
        offered,
        vec![
            (None, crate::views::ALL_PROJECTS),
            (Some(project.id.clone()), project.title.as_str())
        ]
    );
    assert_eq!(selected.map(|c| c.id), Some(None));
    run_message(&mut app, Message::Project(Some(project.id.clone())));
    let (_, selected) = crate::views::project_choices(&app);
    assert_eq!(selected.map(|c| c.id), Some(Some(project.id.clone())));
    // The list of buttons inside the screen is gone: the project's name is the heading
    // alone, and no "모든 프로젝트" button is drawn.
    let body = calendar_texts(crate::views::projects(&app), size);
    assert!(
        !body.iter().any(|s| s == crate::views::ALL_PROJECTS),
        "{body:?}"
    );
    assert!(!body.iter().any(|s| *s == project.title), "{body:?}");
    let labels = calendar_texts(crate::shell::view(&app), size);
    let heading: Vec<_> = labels.iter().take_while(|s| *s != "↻").collect();
    assert_eq!(
        heading.last().map(|s| s.as_str()),
        Some(project.title.as_str())
    );
}

#[test]
fn a_project_shows_capture_only_from_its_own_rows() {
    let mut app = calendar_record_fixture();
    let store = app.store.clone().unwrap();
    let other = store
        .save_project(timeschedule_core::diary::ProjectInput {
            id: None,
            title: "수집 중인 프로젝트".into(),
            description: String::new(),
            repo_path: r"C:\work\other".into(),
        })
        .unwrap();
    let size = Size::new(1160., 1600.);
    let first = app.snapshot.projects[0].id.clone();
    run_message(&mut app, Message::Project(Some(first)));
    // The other project's load fails: the rows on screen are the first project's, so
    // its capture state is unknown and its button waits.
    let _ = app.update(Message::Project(Some(other.id.clone())));
    let _ = app.update(Message::Loaded(
        app.view_generation,
        Err("읽기 실패".into()),
    ));
    let labels = calendar_texts(crate::views::projects(&app), size);
    assert!(labels.iter().any(|s| s == "확인 중"), "{labels:?}");
    let target = calendar_text_bounds(crate::views::projects(&app), size)
        .into_iter()
        .find(|(s, _)| s == "켜기")
        .unwrap()
        .1;
    assert!(click_text(crate::views::projects(&app), size, target).is_empty());
}

#[test]
fn deleting_the_open_project_shows_the_remaining_cards() {
    let mut app = calendar_record_fixture();
    let store = app.store.clone().unwrap();
    let gone = store
        .save_project(timeschedule_core::diary::ProjectInput {
            id: None,
            title: "지울 프로젝트".into(),
            description: String::new(),
            repo_path: String::new(),
        })
        .unwrap();
    run_message(&mut app, Message::Project(Some(gone.id.clone())));
    assert!(app.snapshot.entries.is_empty());
    store.delete_project(&gone.id).unwrap();
    run_message(&mut app, Message::Load);
    assert_eq!(app.project, None);
    // 모든 프로젝트 now shows the cards read again: the deleted one is gone, the other
    // one is there with its record counted.
    let labels = calendar_texts(crate::views::projects(&app), Size::new(1160., 1600.));
    assert!(!labels.iter().any(|s| s == "지울 프로젝트"), "{labels:?}");
    assert!(labels.iter().any(|s| s == "TS"), "{labels:?}");
    assert!(
        labels.iter().any(|s| s.starts_with("기록 1건")),
        "{labels:?}"
    );
}

#[test]
fn a_project_shows_its_repository_and_capture_as_states_and_buttons() {
    let mut app = calendar_record_fixture();
    let store = app.store.clone().unwrap();
    let mut project = app.snapshot.projects[0].clone();
    run_message(&mut app, Message::Project(Some(project.id.clone())));
    let size = Size::new(1160., 1600.);
    let press = |app: &App, label: &str| {
        let target = calendar_text_bounds(crate::views::projects(app), size)
            .into_iter()
            .find(|(s, _)| s == label)
            .unwrap_or_else(|| panic!("{label} is on screen"))
            .1;
        click_text(crate::views::projects(app), size, target)
    };
    let labels = |app: &App| calendar_texts(crate::views::projects(app), size);
    // No repository: its state in words, the hint, and the edit button beside it.
    let shown = labels(&app);
    for label in ["Git 저장소", "미연결", crate::views::NO_REPO_HINT] {
        assert!(shown.iter().any(|s| s == label), "{label}: {shown:?}");
    }
    assert!(!shown.iter().any(|s| s == "채팅 자동 수집"), "{shown:?}");
    assert!(matches!(
        press(&app, "프로젝트 수정").as_slice(),
        [Message::EditForm(_)]
    ));
    // A linked repository with capture off offers to turn it on.
    project.repo_path = r"C:\work\example".into();
    store
        .save_project(timeschedule_core::diary::ProjectInput {
            id: Some(project.id.clone()),
            title: project.title.clone(),
            description: project.description.clone(),
            repo_path: project.repo_path.clone(),
        })
        .unwrap();
    run_message(&mut app, Message::Load);
    let shown = labels(&app);
    for label in ["연결됨", r"C:\work\example", "채팅 자동 수집", "꺼짐"] {
        assert!(shown.iter().any(|s| s == label), "{label}: {shown:?}");
    }
    assert!(!shown.iter().any(|s| s == crate::views::NO_REPO_HINT));
    let id = project.id.clone();
    assert!(matches!(
        press(&app, "켜기").as_slice(),
        [Message::SetChatCapture(p, true)] if *p == id
    ));
    // With capture on, the state reads 켜짐 and the button turns it off.
    app.snapshot.chat_capture_project = Some(project.id.clone());
    let shown = labels(&app);
    assert!(shown.iter().any(|s| s == "켜짐"), "{shown:?}");
    assert!(matches!(
        press(&app, "끄기").as_slice(),
        [Message::SetChatCapture(p, false)] if *p == id
    ));
}

#[test]
fn a_project_shows_one_tab_at_a_time_and_opens_on_its_records() {
    use crate::app::ProjectTab;
    let mut app = calendar_record_fixture();
    let store = app.store.clone().unwrap();
    let project = app.snapshot.projects[0].id.clone();
    run_message(&mut app, Message::Project(Some(project.clone())));
    let size = Size::new(1160., 1600.);
    let labels = |app: &App| calendar_texts(crate::views::projects(app), size);
    // The records tab: its counts, the summary button and the records themselves.
    let shown = labels(&app);
    for label in [
        "기록 · 1",
        "경험 검토 · 0",
        "포트폴리오",
        "근거 요약",
        "+ 연동기록",
    ] {
        assert!(shown.iter().any(|s| s == label), "{label}: {shown:?}");
    }
    assert!(!shown.iter().any(|s| s == "자료 발췌 초안"), "{shown:?}");
    let target = calendar_text_bounds(crate::views::projects(&app), size)
        .into_iter()
        .find(|(s, _)| s == "포트폴리오")
        .unwrap()
        .1;
    assert!(matches!(
        click_text(crate::views::projects(&app), size, target).as_slice(),
        [Message::ProjectTab(ProjectTab::Portfolio)]
    ));
    // The portfolio tab: its draft button, no records.
    let _ = app.update(Message::ProjectTab(ProjectTab::Portfolio));
    let shown = labels(&app);
    assert!(shown.iter().any(|s| s == "자료 발췌 초안"), "{shown:?}");
    assert!(
        !shown.iter().any(|s| s == "+ 연동기록" || s == "근거 요약"),
        "{shown:?}"
    );
    // The review tab says how to get chats when capture is off.
    let _ = app.update(Message::ProjectTab(ProjectTab::Review));
    let shown = labels(&app);
    assert!(
        shown
            .iter()
            .any(|s| s == crate::views::NO_CAPTURE_FOR_REVIEW),
        "{shown:?}"
    );
    // The same project keeps its tab; another project opens on its records.
    run_message(&mut app, Message::Project(Some(project)));
    assert_eq!(app.project_tab, ProjectTab::Review);
    let other = store
        .save_project(timeschedule_core::diary::ProjectInput {
            id: None,
            title: "다른 프로젝트".into(),
            description: String::new(),
            repo_path: String::new(),
        })
        .unwrap();
    run_message(&mut app, Message::Project(Some(other.id)));
    assert_eq!(app.project_tab, ProjectTab::Records);
}

#[test]
fn a_project_never_shows_another_project_s_rows_and_keeps_its_own_state() {
    use crate::app::ProjectTab;
    let mut app = calendar_record_fixture();
    let store = app.store.clone().unwrap();
    let first = app.snapshot.projects[0].id.clone();
    let other = store
        .save_project(timeschedule_core::diary::ProjectInput {
            id: None,
            title: "다른 프로젝트".into(),
            description: String::new(),
            repo_path: String::new(),
        })
        .unwrap();
    let size = Size::new(1160., 1600.);
    let labels = |app: &App| calendar_texts(crate::views::projects(app), size);
    run_message(&mut app, Message::Project(Some(first.clone())));
    assert!(labels(&app).iter().any(|s| s == "+ 연동기록"));
    // A refresh of the same project keeps its rows on screen while it reads.
    let _ = app.update(Message::Load);
    assert!(labels(&app).iter().any(|s| s == "+ 연동기록"), "no flicker");
    run_message(&mut app, Message::Load);
    // Another project whose read fails does not show the first one's record.
    let task = app.update(Message::Project(Some(other.id.clone())));
    assert!(
        !widget_operations(task).is_empty(),
        "another project starts at the top"
    );
    let _ = app.update(Message::Loaded(
        app.view_generation,
        Err("읽기 실패".into()),
    ));
    assert!(
        !labels(&app).iter().any(|s| s == "+ 연동기록"),
        "{:?}",
        labels(&app)
    );
    // The same project chosen again keeps its tab and its portfolio result.
    run_message(&mut app, Message::Project(Some(first.clone())));
    let _ = app.update(Message::ProjectTab(ProjectTab::Portfolio));
    app.portfolio_feedback = Some((first.clone(), "새 v1 생성 완료".into()));
    run_message(&mut app, Message::Project(Some(first.clone())));
    assert_eq!(app.project_tab, ProjectTab::Portfolio);
    assert!(app.portfolio_feedback.is_some());
    // A running draft shows on every tab, not only where it started.
    app.busy = true;
    for tab in [
        ProjectTab::Records,
        ProjectTab::Review,
        ProjectTab::Portfolio,
    ] {
        app.project_tab = tab;
        assert!(labels(&app).iter().any(|s| s == "작업 중…"), "{tab:?}");
    }
    app.busy = false;
    app.project_tab = ProjectTab::Records;
    // Choosing another tab starts it at the top; the same tab moves nothing.
    let task = app.update(Message::ProjectTab(ProjectTab::Records));
    assert!(widget_operations(task).is_empty());
    let task = app.update(Message::ProjectTab(ProjectTab::Review));
    assert!(!widget_operations(task).is_empty());
}

#[test]
fn another_tab_or_project_opens_at_the_top_and_the_same_one_stays() {
    use crate::app::ProjectTab;
    let mut app = calendar_record_fixture();
    let store = app.store.clone().unwrap();
    let first = app.snapshot.projects[0].id.clone();
    for n in 1..=40 {
        let mut form = Form::new(Kind::Entry, app.selected.to_string(), Some(first.clone()));
        form.title = format!("스크롤 기록 {n}");
        form.save(&store).unwrap();
    }
    let other = store
        .save_project(timeschedule_core::diary::ProjectInput {
            id: None,
            title: "다른 프로젝트".into(),
            description: String::new(),
            repo_path: String::new(),
        })
        .unwrap();
    run_message(&mut app, Message::Project(Some(first.clone())));
    let size = Size::new(1160., 820.);
    let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
    let offset = |app: &App, tree: &mut Tree, scroll_to: Option<f32>, ops| {
        let mut element = crate::views::projects(app);
        tree.diff(element.as_widget());
        let node =
            element
                .as_widget_mut()
                .layout(tree, &renderer, &layout::Limits::new(Size::ZERO, size));
        let layout = layout::Layout::new(&node);
        let mut set = SettingsProbe {
            set_scroll: scroll_to,
            set_scroll_index: 0,
            ..Default::default()
        };
        element
            .as_widget_mut()
            .operate(tree, layout, &renderer, &mut set);
        for mut operation in ops {
            let operation: &mut Box<dyn iced::advanced::widget::Operation + Send> = &mut operation;
            element
                .as_widget_mut()
                .operate(tree, layout, &renderer, operation.as_mut());
        }
        let mut probe = SettingsProbe::default();
        element
            .as_widget_mut()
            .operate(tree, layout, &renderer, &mut probe);
        probe.scrolls[0].2.y
    };
    let mut tree = Tree::empty();
    assert!(
        offset(&app, &mut tree, Some(300.), Vec::new()) > 0.,
        "the records scroll"
    );
    // The same tab keeps the place.
    let task = app.update(Message::ProjectTab(ProjectTab::Records));
    let kept = offset(&app, &mut tree, None, widget_operations(task));
    assert!(kept > 0., "{kept}");
    // Another tab opens at its top.
    let task = app.update(Message::ProjectTab(ProjectTab::Portfolio));
    assert_eq!(offset(&app, &mut tree, None, widget_operations(task)), 0.);
    // Back on the records, scrolled, another project opens at the top of its records.
    let _ = app.update(Message::ProjectTab(ProjectTab::Records));
    assert!(offset(&app, &mut tree, Some(300.), Vec::new()) > 0.);
    let task = app.update(Message::Project(Some(other.id)));
    assert_eq!(offset(&app, &mut tree, None, widget_operations(task)), 0.);
}

#[test]
fn all_projects_never_lists_rows_read_for_a_calendar() {
    let mut app = calendar_record_fixture();
    // The month view's rows are on screen; the projects screen's read then fails.
    run_message(&mut app, Message::Navigate(Page::Month));
    let _ = app.update(Message::Navigate(Page::Projects));
    let _ = app.update(Message::Loaded(
        app.view_generation,
        Err("읽기 실패".into()),
    ));
    let labels = calendar_texts(crate::views::projects(&app), Size::new(1160., 1600.));
    assert!(!labels.iter().any(|s| s == "+ 연동기록"), "{labels:?}");
}

#[test]
fn all_projects_shows_a_card_per_project_that_opens_it() {
    let mut app = calendar_record_fixture();
    let store = app.store.clone().unwrap();
    let dir = crate::record_inbox::tests::TestDir::new();
    let captured = store
        .save_project(timeschedule_core::diary::ProjectInput {
            id: None,
            title: "수집 프로젝트".into(),
            description: "채팅을 모으는 프로젝트".into(),
            repo_path: dir.0.to_string_lossy().into_owned(),
        })
        .unwrap();
    // The capture is on for an empty folder: no session is read.
    store.set_chat_capture(&captured.id, true).unwrap();
    run_message(&mut app, Message::Navigate(Page::Projects));
    let size = Size::new(1160., 1600.);
    let labels = calendar_texts(crate::views::projects(&app), size);
    let heading = crate::views::date_heading(
        &app.selected.to_string(),
        chrono::Datelike::year(&chrono::Local::now()),
    );
    for label in [
        "TS".to_owned(),
        format!("기록 1건 · 마지막 {heading}"),
        "저장소 미연결".to_owned(),
        "수집 프로젝트".to_owned(),
        "채팅을 모으는 프로젝트".to_owned(),
        "아직 기록이 없습니다".to_owned(),
        "저장소 연결됨".to_owned(),
        "수집 켜짐".to_owned(),
    ] {
        assert!(labels.iter().any(|s| *s == label), "{label}: {labels:?}");
    }
    // The records themselves live in 기록 보관함, not here.
    assert!(!labels.iter().any(|s| s == "+ 연동기록"), "{labels:?}");
    // A card opens its project.
    let target = calendar_text_bounds(crate::views::projects(&app), size)
        .into_iter()
        .find(|(s, _)| s == "수집 프로젝트")
        .unwrap()
        .1;
    let messages = click_text(crate::views::projects(&app), size, target);
    assert!(
        matches!(messages.as_slice(), [Message::Project(Some(id))] if *id == captured.id),
        "{messages:?}"
    );
}

#[test]
fn project_cards_wait_for_their_own_read_and_say_when_there_are_none() {
    let mut app = fixture();
    run_message(&mut app, Message::Navigate(Page::Projects));
    let size = Size::new(1160., 1600.);
    let labels = calendar_texts(crate::views::projects(&app), size);
    assert!(
        labels.iter().any(|s| s == crate::views::NO_PROJECTS),
        "{labels:?}"
    );
    // Counts read for a calendar are not passed off as this screen's.
    let mut app = calendar_record_fixture();
    run_message(&mut app, Message::Navigate(Page::Month));
    let _ = app.update(Message::Navigate(Page::Projects));
    let _ = app.update(Message::Loaded(
        app.view_generation,
        Err("읽기 실패".into()),
    ));
    let labels = calendar_texts(crate::views::projects(&app), size);
    assert!(labels.iter().any(|s| s == "기록 확인 중"), "{labels:?}");
}

#[test]
fn the_record_menu_is_named_after_its_screen() {
    let mut app = fixture();
    run_message(&mut app, Message::Navigate(Page::Settings));
    let labels = calendar_texts(crate::shell::view(&app), Size::new(1160., 1600.));
    assert!(labels.iter().any(|s| s == "기록 보관함"), "{labels:?}");
    assert!(!labels.iter().any(|s| s == "개발 기록"), "{labels:?}");
}

#[test]
fn project_cards_never_pass_a_failed_read_off_as_current() {
    let mut app = fixture();
    let store = app.store.clone().unwrap();
    let project = store
        .save_project(timeschedule_core::diary::ProjectInput {
            id: None,
            title: "새 프로젝트".into(),
            description: String::new(),
            repo_path: String::new(),
        })
        .unwrap();
    run_message(&mut app, Message::Navigate(Page::Projects));
    let size = Size::new(1160., 1600.);
    let labels = |app: &App| calendar_texts(crate::views::projects(app), size);
    assert!(labels(&app).iter().any(|s| s == "아직 기록이 없습니다"));
    // The first record is saved; the read after it fails: the old "none" is not shown.
    let mut form = Form::new(Kind::Entry, app.selected.to_string(), Some(project.id));
    form.title = "첫 기록".into();
    form.save(&store).unwrap();
    let _ = app.update(Message::Load);
    let _ = app.update(Message::Loaded(
        app.view_generation,
        Err("읽기 실패".into()),
    ));
    let shown = labels(&app);
    assert!(
        !shown.iter().any(|s| s == "아직 기록이 없습니다"),
        "{shown:?}"
    );
    assert!(shown.iter().any(|s| s == "기록 확인 중"), "{shown:?}");
    // The next read succeeds and counts it.
    run_message(&mut app, Message::Load);
    assert!(labels(&app).iter().any(|s| s.starts_with("기록 1건")));
    // A first read that fails does not claim there are no projects.
    let mut app = fixture();
    let _ = app.update(Message::Navigate(Page::Projects));
    let _ = app.update(Message::Loaded(
        app.view_generation,
        Err("읽기 실패".into()),
    ));
    assert!(!labels(&app).iter().any(|s| s == crate::views::NO_PROJECTS));
}

#[test]
fn a_project_card_counts_the_same_records_as_its_list() {
    let mut app = fixture();
    let store = app.store.clone().unwrap();
    let project = store
        .save_project(timeschedule_core::diary::ProjectInput {
            id: None,
            title: "옛 기록".into(),
            description: String::new(),
            repo_path: String::new(),
        })
        .unwrap();
    // A record dated before the lists' range is not counted by the card either.
    for (date, title) in [("1899-12-31", "범위 밖"), ("2026-10-29", "범위 안")] {
        let mut form = Form::new(Kind::Entry, date.into(), Some(project.id.clone()));
        form.title = title.into();
        form.save(&store).unwrap();
    }
    run_message(&mut app, Message::Navigate(Page::Projects));
    let card = app
        .snapshot
        .project_cards
        .get(&project.id)
        .cloned()
        .unwrap();
    assert_eq!(
        (card.records, card.last.as_deref()),
        (1, Some("2026-10-29"))
    );
    run_message(&mut app, Message::Project(Some(project.id)));
    assert_eq!(app.snapshot.entries.len(), 1);
}

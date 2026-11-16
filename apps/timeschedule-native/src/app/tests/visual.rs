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
    assert!(
        labels.iter().any(|s| s == "연동기록"),
        "widget missing saved record: {labels:?}"
    );
    assert!(labels.iter().any(|s| s == "TS"));
}

#[test]
fn diary_calendar_displays_project_and_task_title() {
    let app = calendar_record_fixture();
    let labels = calendar_texts(crate::shell::calendar(&app), Size::new(1200., 1200.));
    assert!(
        labels.iter().any(|s| s == "연동기록"),
        "diary only displays record count: {labels:?}"
    );
    assert!(labels.iter().any(|s| s == "TS"));
}

fn overflowing_calendar_fixture(count: usize) -> App {
    let mut app = calendar_record_fixture();
    for index in 1..count {
        let mut form = Form::new(
            Kind::Entry,
            app.selected.to_string(),
            Some(app.snapshot.projects[0].id.clone()),
        );
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
            assert!(labels.iter().any(|(s, _)| s == &item.title), "{labels:?}");
        }
        let rest = day.items[2..]
            .iter()
            .map(|i| i.title.as_str())
            .collect::<Vec<_>>()
            .join(" · ");
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
fn day_cell_record_is_one_line_with_project_prefix() {
    let app = calendar_record_fixture();
    let day = selected_day(&app);
    for quick in [false, true] {
        let labels = calendar_text_bounds(
            crate::calendar_ui::day_items(&app, Some(day), 160., 60., 12., quick),
            Size::new(160., 60.),
        );
        let project = labels.iter().find(|(s, _)| s == "TS").expect("prefix").1;
        let title = labels
            .iter()
            .find(|(s, _)| s == "연동기록")
            .expect("title")
            .1;
        assert!((project.y - title.y).abs() < 1., "{project:?} {title:?}");
        assert!(project.x < title.x);
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
    assert!(labels.iter().any(|s| s == "연동기록"), "{labels:?}");
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
        .map(|i| i.title.clone())
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
fn incoming_records_and_import_refresh_open_day_and_detail() {
    use crate::calendar_ui::{message, Action};
    for inbox in [true, false] {
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
        if inbox {
            app.records_processing = true;
            run_message(
                &mut app,
                Message::RecordsDrained(Ok(crate::record_inbox::DrainReport {
                    saved: 1,
                    ..Default::default()
                })),
            );
        } else {
            run_message(
                &mut app,
                Message::Imported(Ok(ImportResult {
                    imported: 1,
                    scanned: 1,
                    limited: false,
                })),
            );
        }
        assert_eq!(
            app.calendar.quick.total, 2,
            "open day must refresh, inbox={inbox}"
        );
        assert_eq!(
            app.calendar.detail.as_ref().unwrap().title,
            "외부 수정 반영"
        );
        app.store.as_ref().unwrap().delete_entry(&item.id).unwrap();
        run_message(&mut app, Message::Load);
        assert!(app.calendar.detail.is_none());
    }
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
            vec!["앱을 다시 시작"],
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
                            5,
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

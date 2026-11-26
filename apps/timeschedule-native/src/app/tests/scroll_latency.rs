//! Opt-in release benchmark: actual settings wheel input through software rasterization.
use super::*;
use iced::advanced::graphics::damage;
use std::time::Instant;

#[test]
#[ignore = "timed release benchmark; run explicitly on an otherwise quiet machine"]
fn settings_scroll_response_budget() {
    assert!(!cfg!(debug_assertions), "run this benchmark with --release");
    run_settings_probe(false);
}

#[test]
fn settings_scroll_has_no_stale_pixels() {
    run_settings_probe(true);
}

fn run_settings_probe(verify_only: bool) {
    let frame_count = if verify_only { 2 } else { 50 };
    let mut report = Vec::new();
    // Logical sizes; the large case draws 2880 x 1620 physical pixels. One settings
    // section fits a 1920 x 1080 window, so that case scales a smaller logical window.
    for (width, height, scale, budget_ms) in [
        (800., 620., 1., 33.),
        (1160., 820., 1., 33.),
        (1160., 820., 1.5, 33.),
        (1280., 720., 2.25, 66.),
    ] {
        let size = Size::new(width, height);
        let bounds = Rectangle::new(Point::ORIGIN, size);
        let mut app = fixture();
        app.page = Page::Settings;
        // A long section with no inner scroll under the wheel.
        app.settings_section = crate::app::SettingsSection::Widget;
        let mut renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
        let mut element = app.view(app.manager);
        let mut tree = Tree::new(element.as_widget());
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        let physical = Size::new((width * scale) as u32, (height * scale) as u32);
        let viewport = Viewport::with_physical_size(physical, scale);
        let mut pixels = tiny_skia::Pixmap::new(physical.width, physical.height).unwrap();
        let mut reference = pixels.clone();
        let mut mask = tiny_skia::Mask::new(physical.width, physical.height).unwrap();
        let mut reference_mask = mask.clone();
        let mut previous = Vec::new();
        let mut samples = Vec::new();
        let mut last_offset = 0.;
        let mut moving = 0;
        // Turn the wheel around before the content ends, so every input moves it.
        let mut start_probe = SettingsProbe::default();
        element.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut start_probe,
        );
        let (view, content, _) = *start_probe
            .scrolls
            .iter()
            .find(|s| s.0.x > 150. && s.0.width > 300. && s.0.height > 100.)
            .unwrap();
        let max_offset = content.height - view.height;
        assert!(max_offset >= 120., "{size:?}: the section must scroll");
        let mut down = true;
        let cursor = mouse::Cursor::Available(Point::new(width - 80., height * 0.5));
        for frame in 0..frame_count {
            let start = Instant::now();
            let mut messages = Vec::new();
            let mut shell = iced::advanced::Shell::new(&mut messages);
            if down && last_offset + 60. > max_offset {
                down = false;
            } else if !down && last_offset < 60. {
                down = true;
            }
            let wheel = if down { -1. } else { 1. };
            element.as_widget_mut().update(
                &mut tree,
                &iced::Event::Mouse(mouse::Event::WheelScrolled {
                    delta: mouse::ScrollDelta::Lines { x: 0., y: wheel },
                }),
                layout::Layout::new(&node),
                cursor,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &bounds,
            );
            element.as_widget_mut().update(
                &mut tree,
                &iced::Event::Window(window::Event::RedrawRequested(Instant::now())),
                layout::Layout::new(&node),
                cursor,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &bounds,
            );
            renderer.reset(bounds);
            element.as_widget().draw(
                &tree,
                &mut renderer,
                &app.theme(app.manager),
                &renderer::Style {
                    text_color: crate::theme::palette(app.preferences.theme).text,
                },
                layout::Layout::new(&node),
                cursor,
                &bounds,
            );
            let regions = if previous.is_empty() {
                vec![bounds]
            } else {
                damage::diff(
                    &previous,
                    renderer.layers(),
                    |layer| vec![layer.bounds],
                    iced_tiny_skia::Layer::damage,
                )
            };
            previous = renderer.layers().to_vec();
            let regions = damage::group(regions, bounds);
            renderer.draw(
                &mut pixels.as_mut(),
                &mut mask,
                &viewport,
                &regions,
                crate::theme::palette(app.preferences.theme).background,
            );
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.;
            // Validate scrolling, then compare every frame with a complete redraw.
            // These probes and reference rendering are outside the timed interval.
            let mut probe = SettingsProbe::default();
            element.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut probe,
            );
            let offset = probe
                .scrolls
                .iter()
                .find(|s| s.0.x > 150. && s.0.width > 300. && s.0.height > 100.)
                .unwrap()
                .2
                .y;
            if offset != last_offset {
                moving += 1;
            }
            if frame == 0 {
                assert_eq!(offset, 60., "wheel distance must remain unchanged");
            }
            last_offset = offset;
            assert!(messages.is_empty(), "wheel input must not mutate settings");
            renderer.draw(
                &mut reference.as_mut(),
                &mut reference_mask,
                &viewport,
                &[bounds],
                crate::theme::palette(app.preferences.theme).background,
            );
            assert!(
                pixels.data() == reference.data(),
                "stale pixels at {size:?}, scale {scale}, frame {frame}: regions={regions:?}"
            );
            if verify_only || frame >= 10 {
                samples.push(elapsed_ms);
            }
        }
        assert_eq!(
            moving, frame_count,
            "all wheel inputs must move the settings content"
        );
        samples.sort_by(f64::total_cmp);
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        let p95 = samples[(samples.len() - 1) * 95 / 100];
        eprintln!("SCROLL_LATENCY {width}x{height} @{scale}: mean={mean:.2}ms p95={p95:.2}ms budget={budget_ms}ms");
        report.push(serde_json::json!({"logical": [width, height], "scale": scale, "meanMs": mean, "p95Ms": p95, "budgetMs": budget_ms, "frames": moving}));
    }
    if verify_only {
        return;
    }
    if let Some(output) = std::env::var_os("TIMESCHEDULE_SCROLL_REPORT") {
        std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    assert!(
        report
            .iter()
            .all(|r| r["p95Ms"].as_f64().unwrap() <= r["budgetMs"].as_f64().unwrap()),
        "settings input-to-raster budget exceeded (OS presentation is not included)"
    );
}

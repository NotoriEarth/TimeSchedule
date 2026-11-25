# TimeSchedule software-renderer patch

Source: crates.io `iced_tiny_skia` **0.14.1**, upstream commit
`0ecf60664df7b8ac7d7aef5f7279d5323027f693`, `tiny_skia/` directory in
<https://github.com/iced-rs/iced>. The normalized Cargo manifest and all existing
source files are preserved except `src/lib.rs`; the MIT license is included.

Local change: when damage rectangles cover a summed area greater than 25% of the
viewport, `Renderer::draw` redraws the full viewport once. The sum
is a cost heuristic, not an exact union area. Repeated overlapping regions also
represent repeated work. Empty and sparse updates keep upstream behavior. A large
single region also takes the complete path: a real 150% settings wheel test exposed
stale edge pixels in upstream partial clipping. Existing softbuffer presentation
already presents the complete buffer.

This avoids repeatedly rasterizing large masked backgrounds during settings
scrolls. No event timing, wheel distance, animation, window geometry, background
task, GPU dependency, or user-data behavior changes. The workspace release profile
also compiles `tiny-skia` and `iced_tiny_skia` at `opt-level = 3`; the rest stays at
the existing size-oriented setting.

`src/damage_tests.rs` checks complete redraws at four display scales and preserves
empty/small updates. Run `cargo test -p iced_tiny_skia --lib` from
the repository root. The app's explicit release benchmark exercises real settings
wheel input, checks the unchanged 60px wheel step, and compares every frame with a
complete redraw:

```powershell
cargo test -p timeschedule-native --release --locked --offline settings_scroll_response_budget -- --ignored --nocapture --test-threads=1
```

Its timing ends after CPU rasterization, before Windows presentation. Host-dependent
budgets are 33ms p95 at up to 1160x820 logical pixels and 66ms at 1280x720 logical
pixels / 225% scale (2880x1620 physical pixels); these are input-response checks, not guaranteed display FPS.
Set `TIMESCHEDULE_SCROLL_REPORT` to save the measurements as JSON. The benchmark is
ignored in routine tests because timing assertions on shared CI machines are noisy.

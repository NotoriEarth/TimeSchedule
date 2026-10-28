// A separate executable is necessary because layered child windows require the
// Windows 8+ compatibility manifest. build.rs embeds that manifest for this test
// target; ordinary Rust library unit-test executables do not receive Tauri's .res.
#[cfg(windows)]
#[path = "../src/desktop.rs"]
#[allow(dead_code, unused_imports)]
mod desktop;

#[cfg(windows)]
#[test]
fn dedicated_class_allows_child_alpha_without_changing_parent_class() {
    desktop::verify_layered_child_fixture();
}

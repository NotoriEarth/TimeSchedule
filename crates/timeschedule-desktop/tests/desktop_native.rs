#[cfg(windows)]
#[test]
fn dedicated_class_allows_child_alpha_without_changing_parent_class() {
    timeschedule_desktop::windows::verify_layered_child_fixture();
}

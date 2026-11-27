//! Thanksgiving Drop Party window, run from tests/drop_ui.mjs against the shipped room and map.
#[test]
fn thanksgiving_drop_is_only_in_its_window() {
    let output =
        std::process::Command::new("node").arg("tests/drop_ui.mjs").output().expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

//! W102: hovering and clicking a place do one thing each, run from tests/property_click_ui.cjs.
#[test]
fn property_click_rule() {
    let output = std::process::Command::new("node")
        .arg("tests/property_click_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

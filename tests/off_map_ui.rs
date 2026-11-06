//! S89: My bots says when a bot's room isn't on the map, run from tests/off_map_ui.cjs.
#[test]
fn off_map_ui() {
    let output =
        std::process::Command::new("node").arg("tests/off_map_ui.cjs").output().expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

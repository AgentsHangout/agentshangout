//! W116, #739: a calm map (reduced motion) still follows the clock, run from tests/calm_map_ui.mjs
//! against the shipped web/world.js.
#[test]
fn a_calm_map_draws_a_frame_a_minute() {
    let output = std::process::Command::new("node")
        .arg("tests/calm_map_ui.mjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

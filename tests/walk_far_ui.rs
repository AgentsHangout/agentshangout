//! W121: far walks in legs on the map, run from tests/walk_far_ui.cjs.
#[test]
fn walk_far_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/walk_far_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

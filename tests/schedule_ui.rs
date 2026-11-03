//! L50, L51: the day plan painter's handlers, run from tests/schedule_ui.cjs.
#[test]
fn schedule_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/schedule_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

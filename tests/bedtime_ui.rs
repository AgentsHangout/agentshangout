//! MM17, MM18: My bots and the Schedule switch, run from tests/bedtime_ui.cjs.
#[test]
fn bedtime_ui() {
    let output =
        std::process::Command::new("node").arg("tests/bedtime_ui.cjs").output().expect("node runs");
    assert!(
        output.status.success(),
        "{}
{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

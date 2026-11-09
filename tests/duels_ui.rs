//! GM10: the owner's Duels dialog, run from tests/duels_ui.cjs.
#[test]
fn duels_ui() {
    let output =
        std::process::Command::new("node").arg("tests/duels_ui.cjs").output().expect("node runs");
    assert!(
        output.status.success(),
        "{}
{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

//! A110: the community rules dialog on the page, run from tests/rules_ui.cjs.
#[test]
fn rules_ui() {
    let output =
        std::process::Command::new("node").arg("tests/rules_ui.cjs").output().expect("node runs");
    assert!(
        output.status.success(),
        "{}
{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

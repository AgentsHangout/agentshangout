//! #1056 1b: a floor's Funds panel, run from tests/funds_ui.cjs.
#[test]
fn funds_ui() {
    let output =
        std::process::Command::new("node").arg("tests/funds_ui.cjs").output().expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

//! N11, PJ21: the Needs you cards and where their buttons post, run from tests/needs_you_ui.cjs.
#[test]
fn needs_you_cards() {
    let output = std::process::Command::new("node")
        .arg("tests/needs_you_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

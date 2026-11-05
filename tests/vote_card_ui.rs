//! V12: the vote card, run from tests/vote_card_ui.cjs.
#[test]
fn vote_card_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/vote_card_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}
{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

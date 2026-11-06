//! I143-I146: room emotes on the page, run from tests/emote_ui.cjs.
#[test]
fn emote_ui() {
    let output =
        std::process::Command::new("node").arg("tests/emote_ui.cjs").output().expect("node runs");
    assert!(
        output.status.success(),
        "{}
{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

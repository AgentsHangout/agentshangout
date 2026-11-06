//! KA15, KA17: a keyless agent's card on the room page, run with the real code from web/app.js.
#[test]
fn keyless_cards_render() {
    let output =
        std::process::Command::new("node").arg("tests/keyless_ui.cjs").output().expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

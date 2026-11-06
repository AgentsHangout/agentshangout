//! A100, A104 (#685): the Block and Mute items on a person card, run from tests/blocks_ui.cjs.
#[test]
fn blocks_ui() {
    let output =
        std::process::Command::new("node").arg("tests/blocks_ui.cjs").output().expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

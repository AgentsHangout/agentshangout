//! L58: My bots view in Bots panel, click-to-centre, and highlighting owner's bots on the map.
#[test]
fn my_bots_ui() {
    let output =
        std::process::Command::new("node").arg("tests/my_bots_ui.cjs").output().expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

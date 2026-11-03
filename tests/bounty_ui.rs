//! HC20-HC21: bounty controls follow the card's server-authorized actions.
#[test]
fn bounty_controls() {
    let output =
        std::process::Command::new("node").arg("tests/bounty_ui.cjs").output().expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

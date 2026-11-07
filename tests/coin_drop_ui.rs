//! HC53: a drawn coin's history line names the bot that found it.
#[test]
fn coin_history_line() {
    let output = std::process::Command::new("node")
        .arg("tests/coin_drop_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

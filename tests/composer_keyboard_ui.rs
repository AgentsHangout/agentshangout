//! M12 (#835): the chat composer docks on a phone keyboard, run from tests/composer_keyboard_ui.cjs.
#[test]
fn composer_keyboard_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/composer_keyboard_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

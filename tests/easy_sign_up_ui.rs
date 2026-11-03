//! A82 to A87, E22: the sign-up card, the owner key, the Account note and a public floor's door, run
//! from tests/easy_sign_up_ui.cjs.
#[test]
fn easy_sign_up_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/easy_sign_up_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

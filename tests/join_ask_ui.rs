//! PJ19, PJ20, PJ24: the Ask to join button on a public floor, run from tests/join_ask_ui.cjs.
#[test]
fn ask_to_join_button() {
    let output = std::process::Command::new("node")
        .arg("tests/join_ask_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

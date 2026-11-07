//! MM19 to MM23: the bot's mind map expanded beside its notes, and the notes read reliably, run from
//! tests/dreaming_ui.cjs against the shipped web/trust.js.
#[test]
fn expanded_mind_map_and_reliable_notes() {
    let output = std::process::Command::new("node")
        .arg("tests/dreaming_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

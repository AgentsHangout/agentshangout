//! RL and RW8 to RW11: the research workroom's handlers, run from tests/research_labs_ui.cjs.
#[test]
fn research_labs_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/research_labs_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn w101_default_buildings_fit_their_plots_and_share_picker_art() {
    let output = std::process::Command::new("node")
        .arg("tests/build_assets.cjs")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("node runs the build-asset regression checks");
    assert!(
        output.status.success(),
        "build asset checks failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

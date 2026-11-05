//! W109, #555: hover, click and right click draw one menu, run from tests/game_menu_ui.cjs.
#[test]
fn hover_click_and_right_click_draw_one_menu_with_submenus() {
    let output = std::process::Command::new("node")
        .arg("tests/game_menu_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

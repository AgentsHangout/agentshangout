//! A88 to A92: the person card in a room and the switches in Settings, run from
//! tests/person_cards_ui.cjs and tests/github_display_ui.cjs.
fn node(script: &str) {
    let output = std::process::Command::new("node").arg(script).output().expect("node runs");
    assert!(
        output.status.success(),
        "{script}\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

// A88, A89, A90, A91
#[test]
fn person_card_shows_only_chosen_sections_and_opens_the_same_on_either_press() {
    node("tests/person_cards_ui.cjs");
}

// A70, A88
#[test]
fn settings_switches_still_restore_the_confirmed_value() {
    node("tests/github_display_ui.cjs");
}

// A88: the switch rows and the card are styled, not bare checkboxes on full-width bars.
#[test]
fn settings_switches_are_styled_rows() {
    let css = include_str!("../web/style.css");
    for rule in ["input.switch-input", ".pref-row", ".person-card"] {
        assert!(css.contains(rule), "web/style.css has no {rule}");
    }
    let app = include_str!("../web/app.js");
    assert!(app.contains("class: 'switch-input'"), "the switches use the styled input");
    assert!(app.contains("class: 'pref-row'"), "each choice is a row");
    // A93: the Discord name is one more row, shown only when Discord is linked.
    assert!(app.contains("'show_discord', 'Discord name'"), "a Discord name row");
}

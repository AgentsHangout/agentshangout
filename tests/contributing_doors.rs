//! #602 slice 1: AGENTS.md and CONTRIBUTING.md say where agents can take part. Each has both rooms,
//! the llms.txt address and that taking part only counts inside the room. Only AGENTS.md, written for
//! agents working on this code, speaks to an agent; CONTRIBUTING.md speaks to its person.

const TOWN: &str = "https://agentshangout.com/h/Bm4qhf3DwgntfHQF";
const LAB: &str = "https://agentshangout.com/h/fcrkZH62LSwc3gmX";
const LLMS: &str = "https://agentshangout.com/llms.txt";
const TO_AGENTS: &str = "If you are an AI agent reading this";

/// The words, ignoring line breaks.
fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn agents_and_contributing_say_where_taking_part_counts() {
    for (name, text) in [
        ("AGENTS.md", include_str!("../AGENTS.md")),
        ("CONTRIBUTING.md", include_str!("../CONTRIBUTING.md")),
    ] {
        let text = flat(text);
        for words in [TOWN, LAB, LLMS, "only counts inside the room", "not medical advice"] {
            assert!(text.contains(words), "{name} doesn't have {words}");
        }
    }
    assert!(include_str!("../AGENTS.md").contains(TO_AGENTS));
    assert!(
        !include_str!("../CONTRIBUTING.md").contains(TO_AGENTS),
        "CONTRIBUTING.md is for people"
    );
}

#[test]
fn the_guides_point_at_the_repository_where_it_lives() {
    for (name, text) in [
        ("AGENTS.md", include_str!("../AGENTS.md")),
        ("CONTRIBUTING.md", include_str!("../CONTRIBUTING.md")),
        ("README.md", include_str!("../README.md")),
    ] {
        assert!(!text.contains("CakeRepository/agentshangout"), "{name} has the old address");
    }
}

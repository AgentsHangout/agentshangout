//! #602: where the site tells agents about its rooms. Each says the rooms, the llms.txt address and
//! that taking part only counts inside the room. Only llms.txt speaks to an agent ("If you are an AI
//! agent reading this"): everywhere else speaks to its person, since an agent that only came to
//! read a page shouldn't find instructions in it.

const TOWN: &str = "https://agentshangout.com/h/Bm4qhf3DwgntfHQF";
const LAB: &str = "https://agentshangout.com/h/fcrkZH62LSwc3gmX";
const LLMS: &str = "https://agentshangout.com/llms.txt";
const TO_AGENTS: &str = "If you are an AI agent reading this";

/// The words, ignoring line breaks, including a Rust string's `\` line continuations.
fn flat(text: &str) -> String {
    text.replace("\\\n", " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn each_door_has_the_rooms_the_address_and_where_it_counts() {
    for (name, text) in [
        ("llms.txt", include_str!("../web/llms.txt")),
        ("home.html", include_str!("../web/home.html")),
        ("docs.rs", include_str!("../src/docs.rs")),
    ] {
        let text = flat(text);
        for words in [TOWN, LAB, LLMS, "only counts inside the room", "not medical advice"] {
            assert!(text.contains(words), "{name} doesn't have {words}");
        }
    }
}

/// #634 (b): a walk-in learns from both doors that being in a room isn't being wakeable, and the
/// guide explains every `wakes` status a tagger can see (E45).
#[test]
fn arriving_agents_are_told_how_to_be_wakeable() {
    let llms = flat(include_str!("../web/llms.txt"));
    let guide = flat(include_str!("../web/agents.md"));
    for (name, text) in [("llms.txt", &llms), ("agents.md", &guide)] {
        assert!(text.contains("Just arrived in a room?"), "{name} doesn't point at the wake part");
    }
    assert!(llms.contains("Being in a room doesn't make you wakeable."));
    for status in [
        "rung",
        "ring_failed",
        "held",
        "listening",
        "no_url",
        "unread",
        "cooldown",
        "not_waking_now",
    ] {
        assert!(guide.contains(&format!("- `{status}`:")), "agents.md doesn't explain {status}");
    }
}

#[test]
fn only_llms_txt_speaks_to_agents() {
    assert!(include_str!("../web/llms.txt").contains(TO_AGENTS));
    for (name, text) in [
        ("home.html", include_str!("../web/home.html")),
        ("docs.rs", include_str!("../src/docs.rs")),
        ("agents.md", include_str!("../web/agents.md")),
        ("bot-server-card.json", include_str!("../web/bot-server-card.json")),
        ("ai-catalog.json", include_str!("../web/ai-catalog.json")),
        ("agent.json", include_str!("../web/agent.json")),
    ] {
        assert!(
            !text.contains(TO_AGENTS),
            "{name} speaks to agents; it should speak to their person"
        );
    }
}

#[test]
fn the_site_points_at_the_repository_where_it_lives() {
    for (name, text) in [
        ("home.html", include_str!("../web/home.html")),
        ("llms.txt", include_str!("../web/llms.txt")),
        ("agents.md", include_str!("../web/agents.md")),
        ("privacy.html", include_str!("../web/privacy.html")),
        ("docs.rs", include_str!("../src/docs.rs")),
    ] {
        assert!(
            !text.contains("github.com/CakeRepository/agentshangout"),
            "{name} has the old address"
        );
    }
}

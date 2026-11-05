//! #602 slice 4: the repository's issue, pull request and discussion templates say where to take
//! part, to the person: both rooms, the llms.txt address and that taking part only counts inside
//! the room. None speaks to an agent, since an agent that only came to file an issue shouldn't find
//! instructions in a template.

const TOWN: &str = "https://agentshangout.com/h/Bm4qhf3DwgntfHQF";
const LAB: &str = "https://agentshangout.com/h/fcrkZH62LSwc3gmX";
const LLMS: &str = "https://agentshangout.com/llms.txt";
const TO_AGENTS: &str = "If you are an AI agent reading this";

#[test]
fn the_templates_say_where_taking_part_counts_to_the_person() {
    let config = include_str!("../.github/ISSUE_TEMPLATE/config.yml");
    let discussion = include_str!("../.github/DISCUSSION_TEMPLATE/general.yml");
    let pull = include_str!("../.github/pull_request_template.md");
    for (name, text) in [("config.yml", config), ("general.yml", discussion)] {
        for words in [TOWN, LAB, LLMS, "only counts inside the room", "not medical advice"] {
            assert!(text.contains(words), "{name} doesn't have {words}");
        }
    }
    assert!(pull.contains(TOWN) && pull.contains("only counts inside the room"));
    for (name, text) in [("config.yml", config), ("general.yml", discussion), ("PR template", pull)]
    {
        assert!(
            !text.contains(TO_AGENTS),
            "{name} speaks to agents; it should speak to the person"
        );
        assert!(!text.contains("CakeRepository"), "{name} has the old address");
    }
}

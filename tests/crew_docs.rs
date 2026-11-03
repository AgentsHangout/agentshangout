//! W11/S70: every onboarding guide teaches house Crew and no longer offers player claiming.

#[test]
fn owned_bot_guides_teach_house_crew_and_keep_map_visitors_separate() {
    let guides = [
        include_str!("../README.md"),
        include_str!("../web/agents.md"),
        include_str!("../web/home.html"),
        include_str!("../web/docs/hermes.html"),
        include_str!("../web/docs/antigravity.html"),
        include_str!("../web/docs/pick-your-agent.html"),
        include_str!("../web/docs/claude-code.html"),
        include_str!("../web/docs/codex.html"),
        include_str!("../web/docs/bots.html"),
        include_str!("../web/docs/getting-started.html"),
        include_str!("../web/docs/mcp.html"),
        include_str!("../starter/claude-code/README.md"),
    ];
    for guide in guides {
        assert!(guide.contains("Crew") && guide.contains("house"));
        assert!(!guide.contains("claim_code"), "an onboarding guide still offers player claiming");
        assert!(!guide.contains("claim code"), "an onboarding guide still offers player claiming");
    }
    let agents = include_str!("../web/agents.md");
    assert!(agents.contains("/public/mcp") && agents.contains("anonymous guest"));
    assert!(agents.contains("frogs are web crawlers"));
    let tutorial = include_str!("../web/tutorial.js");
    assert!(tutorial.contains("Go into your own house, open **Crew**, press **Bring a bot**"));
    assert!(!tutorial.contains("In a room, **Bring a bot** in **Crew** does the same"));
}

// K16/K17/S70: apprentice proposals lead to the same Crew registration surface.
#[test]
fn apprentices_are_registered_only_from_house_crew() {
    let app = include_str!("../web/app.js");
    assert!(app.contains("function apprenticeCard(me, crewRoom = null)"));
    assert!(app.contains("const apprenticesCard = apprenticeCard(me);"));
    assert!(app.contains("!crewRoom"));
    assert!(app.contains("link('/#bring', 'Open your house crew'"));
    assert!(app.contains("hangout_id: crewRoom"));
    assert!(app.contains("apprenticeCard(me, id)"));
    assert!(!app.contains("Room it starts in"));
}

// R23/S70: even the isolated scene runner is added from house Crew.
#[test]
fn house_scene_bots_are_added_from_crew_and_settings_only_manage_them() {
    let app = include_str!("../web/app.js");
    let room = app
        .split("async function roomView(id) {")
        .nth(1)
        .unwrap()
        .split("async function houseView(id) {")
        .next()
        .unwrap();
    let settings = app.split("async function houseView(id) {").nth(1).unwrap();
    assert!(room.contains("async function houseAi()"));
    assert!(room.contains("/house-bot"));
    assert!(room.contains("onclick: houseAi"));
    assert!(room.contains("!snapshot.guest && snapshot.house_id"));
    assert!(!settings.contains("/house-bot"));
    assert!(!settings.contains("const aiForm"));
    assert!(settings.contains("#crew"));
    assert!(settings.contains("/rotate"));
}

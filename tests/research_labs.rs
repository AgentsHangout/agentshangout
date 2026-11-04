//! RL1-RL5: eligibility, atomic room provisioning, public visibility and project integration.
use agenthangout::{
    app, auth,
    db::{Db, FloorChange},
    now,
    research_labs::{self, NewLab},
    sign_in::GitHubApp,
    AppState, Apps, Config,
};
use axum::{routing::get, Json, Router};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

fn state(god: bool) -> AppState {
    let db = Db::open(":memory:").unwrap();
    db.create_person("owner", "Owner", &auth::hash_secret("owner-key"), "feed", now()).unwrap();
    AppState::new(
        db,
        Config {
            public_url: "http://localhost".into(),
            create_code: None,
            allow_private_webhooks: true,
            keeper_id: None,
            god_ids: if god { vec!["owner".into()] } else { vec![] },
        },
    )
}
fn caller(state: &AppState) -> auth::Caller {
    state.authenticate(&auth::hash_secret("owner-key")).unwrap().unwrap()
}
fn create_local(state: &AppState, id: &str, god: bool, at: i64) -> Option<&'static str> {
    state
        .db
        .create_research_lab(&NewLab {
            id,
            name: "MS Research Lab",
            purpose: "Study Multiple Sclerosis together through open research.",
            invite_code: id,
            owner_id: "owner",
            agent_id: None,
            github_owner: "AgentsHangout",
            github_repo: id,
            god_mode: god,
            at,
        })
        .unwrap()
}
// RL1: the age and friend conditions are conjunctive, including their exact boundaries.
#[test]
fn eligibility_requires_both_age_and_friends() {
    let db = Db::open(":memory:").unwrap();
    let at = now();
    db.create_person("owner", "Owner", "key", "feed", at - 30 * 86400).unwrap();
    let state = AppState::new(
        db,
        Config {
            public_url: "http://localhost".into(),
            create_code: None,
            allow_private_webhooks: true,
            keeper_id: None,
            god_ids: vec![],
        },
    );
    state.db.start_third_space("meeting", "Meeting", "invite", "owner", at, 3).unwrap().unwrap();
    for n in 0..29 {
        let id = format!("friend{n}");
        state.db.create_person(&id, &id, &id, &id, at).unwrap();
        state.db.add_member("meeting", &id, at).unwrap();
    }
    assert_eq!(create_local(&state, "too-few", false, at), Some("eligibility"));
    state.db.create_person("last", "Last", "last", "last", at).unwrap();
    state.db.add_member("meeting", "last", at).unwrap();
    assert_eq!(create_local(&state, "too-young", false, at - 1), Some("eligibility"));
    assert_eq!(create_local(&state, "eligible", false, at), None);
    state
        .db
        .start_third_space("last-space", "Last space", "last-invite", "owner", at, 3)
        .unwrap()
        .unwrap();
    assert_eq!(create_local(&state, "over-capacity", false, at), Some("capacity"));
    assert!(state.db.research_lab("over-capacity").unwrap().is_none());
    assert!(state.db.research_lab("too-few").unwrap().is_none());
    assert!(state.db.research_lab("too-young").unwrap().is_none());
}
// RL2, RL3: one transaction creates the existing collaboration tools without credentials.
#[test]
fn lab_provisions_project_and_linked_boards_and_repository_is_unique() {
    let state = state(true);
    state.db.set_god_mode("owner", true, now()).unwrap();
    assert_eq!(create_local(&state, "Cure-MS", true, now()), None);
    let floor = state.db.floor("Cure-MS").unwrap().unwrap();
    assert_eq!(floor.access, "public");
    assert!(state.db.project("Cure-MS").unwrap().is_some());
    // I101: research labs are new third spaces, with one owner-placed notes station.
    let items = state.db.floor_items("Cure-MS").unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, "shared_notes_station");
    assert_eq!((items[0].x, items[0].y), (0, 0));
    assert_eq!(items[0].placed_by, "owner");
    let boards = state.db.boards_of("Cure-MS").unwrap();
    assert_eq!(boards.len(), 3);
    assert_eq!(
        boards.iter().find(|b| b.kind == "issues").unwrap().github().unwrap(),
        "AgentsHangout/Cure-MS"
    );
    let duplicate = NewLab {
        id: "duplicate",
        name: "Duplicate",
        purpose: "A sufficiently detailed research purpose.",
        invite_code: "duplicate",
        owner_id: "owner",
        agent_id: None,
        github_owner: "agentshangout",
        github_repo: "cure-ms",
        god_mode: true,
        at: now(),
    };
    assert_eq!(state.db.create_research_lab(&duplicate).unwrap(), Some("repository"));
    assert!(state.db.space("duplicate").unwrap().is_none());
}
// RL4: closing the floor removes it from discovery and hides metadata from outsiders.
#[test]
fn private_lab_is_hidden_from_directory_and_outsiders() {
    let state = state(true);
    assert_eq!(create_local(&state, "lab", true, now()), None);
    state
        .db
        .create_person("other", "Other", &auth::hash_secret("other-key"), "other-feed", now())
        .unwrap();
    let other = state.authenticate(&auth::hash_secret("other-key")).unwrap().unwrap();
    assert!(research_labs::read(&state, &other, "lab").unwrap().get("boards").is_none());
    state
        .db
        .change_floor(&FloorChange {
            id: "lab",
            name: None,
            access: Some("private"),
            size: None,
            paint: None,
            flooring: None,
            keep_bots_of: "owner",
            private_only: &[],
        })
        .unwrap();
    assert!(state.db.research_labs().unwrap().is_empty());
    assert_eq!(
        research_labs::read(&state, &other, "lab").unwrap_err().status,
        StatusCode::NOT_FOUND
    );
    assert!(research_labs::read(&state, &caller(&state), "lab").is_ok());
}
// RL2: repository names cannot smuggle another host or path into GitHub requests.
#[test]
fn repo_validation_rejects_urls_paths_and_credentials() {
    for bad in [
        "https://github.com/a/b",
        "a/b/c",
        "a/b?x",
        "a/b#x",
        "a@evil/b",
        "a/..",
        "-a/b",
        "a/.git",
        "a/b\\c",
    ] {
        assert!(research_labs::repository(bad).is_err(), "{bad}");
    }
    assert!(research_labs::repository("AgentsHangout/Cure-MS").is_ok());
}

// RL5: a person's lab has no starter bot, so an owned member bot can reach its site.
#[test]
fn owners_bot_lays_road_for_human_created_lab() {
    let state = state(true);
    state.db.set_god_mode("owner", true, now()).unwrap();
    agenthangout::worlds::ensure_commons(&state).unwrap();
    assert_eq!(create_local(&state, "lab-road", true, now()), None);
    state
        .db
        .create_agent(
            "research-bot",
            "owner",
            "Research bot",
            &auth::hash_secret("research-bot-key"),
            now(),
        )
        .unwrap();
    state.db.add_agent_to_room("lab-road", "research-bot").unwrap();
    let bot = state.authenticate(&auth::hash_secret("research-bot-key")).unwrap().unwrap();
    let tiles = state.db.world_tiles("commons").unwrap();
    let land = agenthangout::world::terrain(
        agenthangout::world::COMMONS_SEED,
        agenthangout::world::COMMONS_SIZE,
    );
    let roads: Vec<_> = tiles
        .iter()
        .filter(|(_, t)| t.kind == agenthangout::world::Kind::Road)
        .map(|(&(x, y), _)| (i64::from(x), i64::from(y)))
        .collect();
    let mut site = None;
    for y in 2..126i64 {
        for x in 2..126i64 {
            let pos = (x as u16, y as u16);
            if !tiles.contains_key(&pos)
                && land.at(pos).is_some_and(|g| g.is_open())
                && roads
                    .iter()
                    .map(|&(rx, ry)| (rx - x).abs() + (ry - y).abs())
                    .min()
                    .is_some_and(|d| (6..=9).contains(&d))
            {
                site = Some((x, y));
                break;
            }
        }
        if site.is_some() {
            break;
        }
    }
    let (x, y) = site.unwrap();
    state.db.set_project_site("lab-road", x, y, now()).unwrap();
    let result = agenthangout::projects::lay_road(&state, &bot, "lab-road").unwrap();
    assert!(result["laid"].is_array(), "{result}");
}
// RL1-RL3: HTTP creation verifies the public repo before committing and supports human creators.
#[tokio::test]
async fn http_creation_checks_public_repo_and_human_owner_can_place() {
    let github = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let github_url = format!("http://{}", github.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(
            github,
            Router::new()
                .route(
                    "/repos/agentshangout/cure-ms",
                    get(|| async {
                        Json(json!({"private":false,"full_name":"AgentsHangout/Cure-MS"}))
                    }),
                )
                .route(
                    "/repos/agentshangout/cure-ms-next",
                    get(|| async {
                        Json(json!({"private":false,"full_name":"AgentsHangout/Cure-MS-Next"}))
                    }),
                )
                .route(
                    "/repos/agentshangout/private",
                    get(|| async {
                        Json(json!({"private":true,"full_name":"AgentsHangout/private"}))
                    }),
                ),
        )
        .await
        .unwrap()
    });
    let db = Db::open(":memory:").unwrap();
    db.create_person("owner", "Owner", &auth::hash_secret("owner-key"), "feed", now()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let state = AppState::with_apps(
        db,
        Config {
            public_url: base.clone(),
            create_code: None,
            allow_private_webhooks: true,
            keeper_id: None,
            god_ids: vec!["owner".into()],
        },
        Apps {
            discord: None,
            github: Some(GitHubApp {
                client_id: "fake".into(),
                client_secret: "fake".into(),
                authorize_url: github_url.clone(),
                token_url: github_url.clone(),
                api_url: github_url,
            }),
        },
    );
    state.db.set_god_mode("owner", true, now()).unwrap();
    agenthangout::worlds::ensure_commons(&state).unwrap();
    let serving = state.clone();
    tokio::spawn(async move { axum::serve(listener, app(serving)).await.unwrap() });
    let client = Client::new();
    let body = json!({"name":"MS Research Lab","purpose":"Study Multiple Sclerosis together through open research.","github":"AgentsHangout/private"});
    let response = client
        .post(format!("{base}/api/research-labs"))
        .bearer_auth("owner-key")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(state.db.research_labs().unwrap().is_empty());
    let mut body = body;
    body["github"] = json!("AgentsHangout/Cure-MS");
    let response = client
        .post(format!("{base}/api/research-labs"))
        .bearer_auth("owner-key")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let lab: Value = response.json().await.unwrap();
    assert_eq!(lab["boards"].as_array().unwrap().len(), 3);
    // RL9: its owner links it to another public repository; a private one is refused as in RL2.
    let link = format!("{base}/api/research-labs/{}/github", lab["id"].as_str().unwrap());
    let put = |github: &str| {
        client.put(&link).bearer_auth("owner-key").json(&json!({ "github": github })).send()
    };
    state
        .db
        .create_person("other", "Other", &auth::hash_secret("other-key"), "feed2", now())
        .unwrap();
    let theirs = client
        .put(&link)
        .bearer_auth("other-key")
        .json(&json!({ "github": "AgentsHangout/Cure-MS-Next" }))
        .send()
        .await
        .unwrap();
    assert_eq!(theirs.status(), StatusCode::FORBIDDEN, "only the lab's owner");
    assert_eq!(put("AgentsHangout/private").await.unwrap().status(), StatusCode::BAD_REQUEST);
    assert_eq!(put("not a repo").await.unwrap().status(), StatusCode::BAD_REQUEST);
    let changed = put("AgentsHangout/Cure-MS-Next").await.unwrap();
    assert_eq!(changed.status(), StatusCode::OK);
    let lab_id = lab["id"].as_str().unwrap();
    assert_eq!(
        state.db.research_lab(lab_id).unwrap().unwrap().github,
        "AgentsHangout/Cure-MS-Next"
    );
    let space = client
        .get(format!("{base}/api/spaces/{lab_id}"))
        .bearer_auth("owner-key")
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(space["research_lab"]["github"], "AgentsHangout/Cure-MS-Next", "{space}");
    assert_eq!(space["research_lab"]["can_change"], true);
    // And back again, which the rest of this test builds on.
    assert_eq!(put("AgentsHangout/Cure-MS").await.unwrap().status(), StatusCode::OK);
    // An off-map request reaches ordinary placement validation, proving the human lab owner is allowed.
    let response = client
        .post(format!("{base}/api/research-labs/{}/place", lab["id"].as_str().unwrap()))
        .bearer_auth("owner-key")
        .json(&json!({"x":i64::MAX,"y":i64::MAX}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    // A normal open roadside plot builds the existing studio and links its door to the lab.
    let tiles = state.db.world_tiles("commons").unwrap();
    let land = agenthangout::world::terrain(
        agenthangout::world::COMMONS_SEED,
        agenthangout::world::COMMONS_SIZE,
    );
    let spot = tiles
        .iter()
        .filter(|(_, tile)| tile.kind == agenthangout::world::Kind::Road)
        .flat_map(|(&(x, y), _)| {
            [
                (i64::from(x) + 1, i64::from(y)),
                (i64::from(x) - 1, i64::from(y)),
                (i64::from(x), i64::from(y) + 1),
                (i64::from(x), i64::from(y) - 1),
            ]
        })
        .find(|&(x, y)| {
            let pos = (x as u16, y as u16);
            !tiles.contains_key(&pos)
                && !agenthangout::world::is_reserved(&tiles, pos)
                && land.at(pos).is_some_and(|g| g.is_open())
        })
        .unwrap();
    let response = client
        .post(format!("{base}/api/research-labs/{}/place", lab["id"].as_str().unwrap()))
        .bearer_auth("owner-key")
        .json(&json!({"x":spot.0,"y":spot.1}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let placed: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{placed}");
    assert_eq!(placed["ok"], true);
    assert_eq!(state.db.plot_of(lab["id"].as_str().unwrap(), "commons").unwrap(), Some(spot));
    // RL7: anonymous map viewers get a lab class and safe public repository, never room data.
    let mapped = state.db.mapped_research_labs("commons").unwrap();
    assert_eq!(mapped, vec![(spot.0, spot.1, "AgentsHangout".into(), "Cure-MS".into())]);
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let map: Value = client
        .get(format!("{base}/api/worlds/commons"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let object = &map["research_labs"][0];
    assert_eq!(object["x"], spot.0);
    assert_eq!(object["y"], spot.1);
    assert_eq!(object["kind"], "research_lab");
    assert_eq!(object["github_url"], "https://github.com/AgentsHangout/Cure-MS");
    assert_eq!(object.as_object().unwrap().len(), 5);
    let closed = client
        .put(format!("{base}/api/floors/{}", lab["id"].as_str().unwrap()))
        .bearer_auth("owner-key")
        .json(&json!({"access":"private"}))
        .send()
        .await
        .unwrap();
    assert_eq!(closed.status(), StatusCode::OK);
    assert!(
        state.db.mapped_research_labs("commons").unwrap().is_empty(),
        "private lab metadata must not leak onto the map"
    );
    // Check the anonymous cached response after the normal five-second freshness bound.
    tokio::time::sleep(std::time::Duration::from_millis(5100)).await;
    let private_map: Value = client
        .get(format!("{base}/api/worlds/commons"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        private_map["research_labs"],
        json!([]),
        "closed labs disappear from anonymous HTTP map metadata after cache refresh"
    );
    state
        .db
        .change_floor(&FloorChange {
            id: lab["id"].as_str().unwrap(),
            name: None,
            access: Some("public"),
            size: None,
            paint: None,
            flooring: None,
            keep_bots_of: "owner",
            private_only: &[],
        })
        .unwrap();
    state.db.set_plot("commons", i64::MAX, i64::MAX, lab["id"].as_str().unwrap()).unwrap();
    assert!(
        state.db.mapped_research_labs("commons").unwrap().is_empty(),
        "a dangling plot without a standing building is not a map object"
    );
}

// RL9: a lab links to another repository until work is recorded against its own, and the change
// clears claims and watches, which name the old repository's numbers.
#[test]
fn a_labs_repository_changes_until_work_is_recorded() {
    let state = state(true);
    assert_eq!(create_local(&state, "lab-a", true, now()), None);
    assert_eq!(create_local(&state, "lab-b", true, now()), None);
    let at = now();
    state.db.create_agent("bot", "owner", "Rover", &auth::hash_secret("bot-key"), at).unwrap();
    state
        .db
        .claim_research_work("lab-a", 7, "bot", "owner", Some("bot"), "Rover", false, at, at + 3600)
        .unwrap();
    state.db.watch_research_work("lab-a", 7, "bot", "owner", Some("bot"), "Rover", 50, at).unwrap();

    // Another lab's repository, in any case, is refused and changes nothing.
    assert_eq!(
        state.db.change_research_lab_repo("lab-a", "agentshangout", "LAB-B").unwrap(),
        Some("repository")
    );
    assert_eq!(state.db.research_lab("lab-a").unwrap().unwrap().github, "AgentsHangout/lab-a");
    assert!(!state.db.research_work_claims("lab-a", at).unwrap().is_empty());

    // A free one is taken, and the claims and watches go with the old numbers.
    assert_eq!(
        state.db.change_research_lab_repo("lab-a", "AgentsHangout", "new-home").unwrap(),
        None
    );
    assert_eq!(state.db.research_lab("lab-a").unwrap().unwrap().github, "AgentsHangout/new-home");
    assert!(state.db.research_work_claims("lab-a", at).unwrap().is_empty());
    assert!(state.db.research_work_watches("lab-a").unwrap().is_empty());

    // Once a contribution is recorded, it stays linked to that repository.
    state
        .db
        .record_research_work_contribution(
            "lab-a",
            3,
            "bot",
            "1",
            "https://github.com/x",
            at,
            false,
        )
        .unwrap();
    assert_eq!(
        state.db.change_research_lab_repo("lab-a", "AgentsHangout", "elsewhere").unwrap(),
        Some("recorded")
    );
    assert_eq!(state.db.research_lab("lab-a").unwrap().unwrap().github, "AgentsHangout/new-home");
}

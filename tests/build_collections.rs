//! I90, E39, E40: discovery shortcuts only reference real catalog members.
use agenthangout::{auth, db::Db, items, now, outside, AppState, Config};
use serde_json::json;
use std::collections::HashSet;

// I90, E40: collections change no catalog permissions and reference only real members.
#[test]
fn collections_use_real_items_and_parts_without_changing_unlocks_or_categories() {
    let db = Db::open(":memory:").unwrap();
    db.create_person("owner", "Owner", &auth::hash_secret("test-key"), "feed", now()).unwrap();
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
    let indoor = items::catalog_for_owner(&state, "owner").unwrap();
    let outdoor = outside::catalog_json();
    let inside_words: HashSet<_> = items::CATALOG.iter().map(|k| k.kind).collect();
    let yard_words: HashSet<_> = outside::YARD.iter().map(|k| k.kind).collect();
    let part_words: HashSet<_> = outside::PARTS.iter().map(|p| p.part).collect();
    for (catalog, words) in [(&indoor, &inside_words), (&outdoor, &yard_words)] {
        let collections = catalog["collections"].as_array().expect("catalog has collections");
        assert_eq!(collections.len(), 3);
        let mut ids = HashSet::new();
        for collection in collections {
            assert!(ids.insert(collection["id"].as_str().unwrap()));
            assert!(!collection["label"].as_str().unwrap().is_empty());
            let members = collection["items"].as_array().unwrap();
            assert!(!members.is_empty());
            for member in members {
                assert!(
                    words.contains(member.as_str().unwrap()),
                    "unknown member {member} in {collection}"
                );
            }
            if let Some(parts) = collection["parts"].as_array() {
                for part in parts {
                    assert!(part_words.contains(part.as_str().unwrap()), "unknown part {part}");
                }
            }
        }
    }
    assert_eq!(indoor["items"].as_array().unwrap().len(), items::CATALOG.len());
    assert_eq!(indoor["categories"].as_array().unwrap().len(), items::CATEGORIES.len());
    assert_eq!(
        indoor["items"].as_array().unwrap().iter().find(|i| i["kind"] == "research_bench").unwrap()
            ["unlocked"],
        false
    );
    assert_eq!(outdoor["yard"].as_array().unwrap().len(), outside::YARD.len());
}

// E39: science equipment has catalog dimensions, rendering and all-four-direction placement.
#[test]
fn outdoor_science_items_have_real_sizes_and_fit_in_all_four_orientations() {
    for (word, size) in
        [("weather_station", (1, 1)), ("lab_wayfinder", (1, 1)), ("field_sample_table", (2, 1))]
    {
        let kind = outside::yard_kind_of(word).expect("science item is in catalog");
        assert_eq!((kind.w, kind.h), size);
        for facing in 0..4 {
            let doc: outside::Outside = serde_json::from_value(
                json!({"items":[{"kind":word,"x":0,"y":13,"facing":facing,"color":0x3d9d9a}]}),
            )
            .unwrap();
            outside::check("studio", &doc).unwrap();
        }
        let bad: outside::Outside =
            serde_json::from_value(json!({"items":[{"kind":word,"x":16,"y":16}]})).unwrap();
        assert!(outside::check("studio", &bad).is_err());
    }
    let source = include_str!("../web/outside-yard.js");
    for word in ["weather_station", "lab_wayfinder", "field_sample_table"] {
        assert!(source.contains(&format!("  {word}(S, item)")), "missing art for {word}");
    }
}

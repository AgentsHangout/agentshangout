use serde_json::Value;

/// The last line of camera-render.js, which draws the photo.
const DRAW: &str = "JSON.stringify(cameraSvg(JSON.parse(__camera_scene)));";

/// A rendered photo and the ids of the bots drawn at an item, at the table and at a desk in it.
pub struct Photo {
    pub png: Vec<u8>,
    pub at_items: Vec<String>,
    pub at_table: Vec<String>,
    pub at_desk: Vec<String>,
}

#[derive(serde::Deserialize)]
struct Drawn {
    svg: String,
    at: Vec<String>,
    #[serde(default)]
    table: Vec<String>,
    #[serde(default)]
    desk: Vec<String>,
}

/// Runs the room art and camera-render.js on a scene, in a bounded runtime. `last` replaces the
/// script's final line, which draws the photo.
fn run(scene: &Value, last: Option<&str>) -> Result<String, String> {
    let runtime = rquickjs::Runtime::new().map_err(|e| e.to_string())?;
    runtime.set_memory_limit(64 * 1024 * 1024);
    runtime.set_max_stack_size(512 * 1024);
    let started = std::time::Instant::now();
    runtime.set_interrupt_handler(Some(Box::new(move || {
        started.elapsed() > std::time::Duration::from_secs(2)
    })));
    let context = rquickjs::Context::full(&runtime).map_err(|e| e.to_string())?;
    let art = include_str!("../../web/room.js")
        .lines()
        .map(|line| line.strip_prefix("export ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    let camera = include_str!("../../web/camera-render.js");
    let camera = match last {
        Some(last) => {
            let (rest, end) = camera.trim_end().rsplit_once('\n').ok_or("empty camera script")?;
            if end != DRAW {
                return Err(format!("camera-render.js must end with {DRAW}"));
            }
            format!("{rest}\n{last}")
        }
        None => camera.to_string(),
    };
    let script = format!("{art}\n{camera}");
    context
        .with(|ctx| {
            ctx.globals().set("__camera_scene", scene.to_string())?;
            ctx.eval::<String, _>(script)
        })
        .map_err(|e| format!("camera drawing failed: {e}"))
}

pub fn png(scene: &Value) -> Result<Photo, String> {
    let drawn: Drawn = serde_json::from_str(&run(scene, None)?).map_err(|e| e.to_string())?;
    let svg = drawn.svg;
    if svg.len() > 4 * 1024 * 1024 {
        return Err("camera SVG is too large".into());
    }
    let mut options = resvg::usvg::Options::default();
    options.image_href_resolver.resolve_data = Box::new(|_, _, _| None);
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    options
        .fontdb_mut()
        .load_font_data(include_bytes!("../../assets/fonts/DejaVuSans.ttf").to_vec());
    let tree = resvg::usvg::Tree::from_str(&svg, &options).map_err(|e| e.to_string())?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(960, 504).ok_or("camera allocation failed")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::identity(), &mut pixmap.as_mut());
    let bytes = pixmap.encode_png().map_err(|e| e.to_string())?;
    if bytes.len() > 512 * 1024 {
        return Err("camera PNG is too large".into());
    }
    Ok(Photo { png: bytes, at_items: drawn.at, at_table: drawn.table, at_desk: drawn.desk })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Where each figure ends up, in scene order: [name, x, y, pose].
    fn places(scene: &Value) -> Vec<(String, f64, f64, String)> {
        let out = run(scene, Some("const s=JSON.parse(__camera_scene);JSON.stringify(cameraFigures(s,cameraPlaced(s)).map(f=>[f.name,f.x,f.y,f.pose]))")).unwrap();
        serde_json::from_str(&out).unwrap()
    }
    fn room(items: Value, bots: Value, frame: Value) -> Value {
        json!({"look":{"paint":"dusk","flooring":"oak","hidden":[],"outdoors":false,"table":null},
               "items":items,"bots":bots,"people":[],"frame":frame})
    }
    fn whole() -> Value {
        json!({"x":0,"y":0,"width":480,"height":252})
    }
    fn bot(name: &str, using: Option<&str>) -> Value {
        json!({"id":name,"name":name,"color":"#8866cc","look":{},"using":using})
    }

    fn bot_in(name: &str, state: &str, working: bool) -> Value {
        json!({"id":name,"name":name,"color":"#8866cc","look":{},"using":null,"state":state,"working":working})
    }

    // CP9: as on the room page, an active bot sits at the table and one working through another tool
    // sits at a desk, while an idle one stands in the grid. One using an item is still at it.
    #[test]
    fn active_bots_sit_at_the_table_and_working_ones_at_a_desk() {
        let items = json!([{"id":"p","item":"piano","x":30,"y":8,"w":4,"h":2,"facing":0}]);
        let mut piano = bot_in("d", "active", false);
        piano["using"] = json!("p");
        let bots = json!([
            bot_in("a", "active", false),
            bot_in("b", "idle", false),
            bot_in("c", "active", true),
            piano
        ]);
        let all = places(&room(items, bots, whole()));
        let by = |name: &str| all.iter().find(|f| f.0 == name).cloned().unwrap();
        // The table's first seat, as SEATS has it with the table where it starts.
        assert_eq!(by("a"), ("a".into(), 192.0, 127.0, "sit".into()), "{all:?}");
        assert_eq!(
            by("c"),
            ("c".into(), 44.0, 146.0, "sit".into()),
            "a working bot is at the first desk: {all:?}"
        );
        assert_eq!(by("b").3, "stand", "an idle bot stays in the grid: {all:?}");
        assert_eq!(by("d").3, "sit", "the piano user is still at the piano: {all:?}");
        assert_ne!((by("d").1, by("d").2), (192.0, 127.0));

        let scene = room(
            json!([]),
            json!([bot_in("a", "active", false), bot_in("c", "active", true)]),
            whole(),
        );
        let photo = png(&scene).unwrap();
        assert_eq!(photo.at_table, ["a"]);
        assert_eq!(photo.at_desk, ["c"]);
        assert!(photo.at_items.is_empty());
    }

    // CP9: a moved table moves its seats, and a seat outside the frame leaves the bot standing.
    #[test]
    fn table_seats_follow_the_table_and_the_frame() {
        let mut moved = room(json!([]), json!([bot_in("a", "active", false)]), whole());
        moved["look"]["table"] = json!({"x":14,"y":1,"style":"round"});
        let seated = places(&moved);
        assert_eq!(
            (seated[0].1, seated[0].2, seated[0].3.as_str()),
            (192.0 + 24.0, 127.0, "sit"),
            "{seated:?}"
        );
        let left = json!({"x":0,"y":0,"width":150,"height":168});
        let outside = places(&room(json!([]), json!([bot_in("a", "active", false)]), left));
        assert_eq!(outside[0].3, "stand", "{outside:?}");
    }

    #[test]
    fn a_bot_using_an_item_is_drawn_at_it_and_the_rest_stand_in_the_grid() {
        let items = json!([{"id":"p","item":"piano","x":30,"y":8,"w":4,"h":2,"facing":0}]);
        let alone = places(&room(items.clone(), json!([bot("b", None)]), whole()));
        let scene = room(items, json!([bot("a", Some("p")), bot("b", None)]), whole());
        let both = places(&scene);
        assert_eq!(both[0].3, "sit");
        assert!(both[0].1 > 30.0 * 12.0 && both[0].1 < 34.0 * 12.0, "{both:?}");
        // The bot at the piano isn't in the grid, so the other one stands where it would alone.
        assert_eq!((both[1].1, both[1].2, &both[1].3), (alone[0].1, alone[0].2, &alone[0].3));
        // The same scene always gives the same places.
        assert_eq!(places(&scene), both);
        assert_eq!(png(&scene).unwrap().at_items, ["a"]);
    }

    #[test]
    fn missing_full_or_out_of_frame_items_leave_a_bot_standing() {
        let items = json!([{"id":"p","item":"piano","x":30,"y":8,"w":4,"h":2,"facing":0},
                           {"id":"rock","item":"no_such_kind","x":3,"y":3,"w":1,"h":1,"facing":0}]);
        // Moved or deleted (no such id), a kind the page doesn't draw, and a second bot at a
        // one-seat piano all stand in the grid with a neutral pose.
        let bots = json!([
            bot("a", Some("p")),
            bot("b", Some("p")),
            bot("c", Some("gone")),
            bot("d", Some("rock"))
        ]);
        let all = places(&room(items.clone(), bots, whole()));
        assert_eq!(all[0].3, "sit");
        assert!(all[1..].iter().all(|f| f.3 == "stand"), "{all:?}");
        // Framed away from the piano, its user stands in the grid too.
        let left = json!({"x":0,"y":84,"width":320,"height":168});
        let framed = places(&room(items.clone(), json!([bot("a", Some("p"))]), left.clone()));
        assert_eq!(framed[0].3, "stand");
        assert!(framed[0].1 < 320.0);
        assert!(png(&room(items, json!([bot("a", Some("p"))]), left)).unwrap().at_items.is_empty());
    }

    #[test]
    fn a_sofa_seats_as_many_as_it_has_room_for_without_overlap() {
        let items = json!([{"id":"s","item":"sofa","x":10,"y":6,"w":4,"h":2,"facing":0}]);
        let bots = json!([
            bot("a", Some("s")),
            bot("b", Some("s")),
            bot("c", Some("s")),
            bot("d", Some("s"))
        ]);
        let scene = room(items, bots, whole());
        let all = places(&scene);
        let seated: Vec<_> = all.iter().filter(|f| f.3 == "sit").collect();
        assert_eq!(seated.len(), 3, "{all:?}");
        for (i, a) in seated.iter().enumerate() {
            assert!(seated[i + 1..].iter().all(|b| (a.1, a.2) != (b.1, b.2)), "{all:?}");
        }
        assert_eq!(all[3].3, "stand");
        png(&scene).unwrap();
    }

    #[test]
    fn the_test_helper_refuses_a_script_with_a_different_last_line() {
        let camera = include_str!("../../web/camera-render.js");
        assert_eq!(camera.trim_end().rsplit_once('\n').unwrap().1, DRAW);
    }

    #[test]
    fn a_spot_on_the_frame_edge_is_out_of_frame() {
        // Framed so the stool's spot is just inside the frame's left edge.
        let items = json!([{"id":"s","item":"stool","x":20,"y":6,"w":1,"h":1,"facing":0}]);
        let spot = places(&room(items.clone(), json!([bot("a", Some("s"))]), whole()));
        assert_eq!(spot[0].3, "sit");
        let x = spot[0].1;
        let edge = json!({"x":x - 4.0,"y":84,"width":320,"height":168});
        assert_eq!(places(&room(items, json!([bot("a", Some("s"))]), edge))[0].3, "stand");
    }

    #[test]
    fn busy_items_draw_their_users_in_a_photo() {
        for kind in ["piano", "jukebox", "easel", "game_table", "fireplace", "armchair"] {
            let items = json!([{"id":"i","item":kind,"x":12,"y":6,"w":3,"h":2,"facing":0}]);
            png(&room(items, json!([bot("a", Some("i"))]), whole())).unwrap();
        }
    }
    #[test]
    fn room_art_renders_as_a_bounded_png() {
        let image = png(&serde_json::json!({
            "look":{"paint":"dusk","flooring":"oak","hidden":[],"outdoors":false,"table":null},
            "items":[], "bots":[{"id":"bot","name":"Dotliver Tree","color":"#8866cc","look":{}}],
            "frame":{"x":0,"y":0,"width":480,"height":252}
        }))
        .unwrap()
        .png;
        assert_eq!(&image[..8], b"\x89PNG\r\n\x1a\n");
        assert!(image.len() < 512 * 1024);
    }
    #[test]
    fn decorated_outdoor_room_and_xml_like_names_render_without_external_resources() {
        let scene = serde_json::json!({
            "look":{"paint":"sage","flooring":"grass","hidden":["bar"],"outdoors":true,"table":{"x":10,"y":1,"style":"round"}},
            "items":[{"id":"table","item":"coffee_table","x":3,"y":7,"w":3,"h":2,"facing":1},
                     {"id":"piano","item":"piano","x":30,"y":8,"w":4,"h":2,"facing":2}],
            "bots":[{"id":"one","name":"Bot\u{1} <&\" Ω","color":"#8866cc","look":{},"using":"piano"},
                    {"id":"two","name":"morgbot 2.0","color":"#44aaff","look":{}},
                    {"id":"three","name":"AgentsHangout PR","color":"#ff8866","look":{}}],
            "frame":{"x":80,"y":60,"width":320,"height":168}
        });
        let bytes = png(&scene).unwrap().png;
        let pixels = resvg::tiny_skia::Pixmap::decode_png(&bytes).unwrap();
        assert_eq!((pixels.width(), pixels.height()), (960, 504));
        let colors: std::collections::HashSet<_> =
            pixels.data().as_chunks::<4>().0.iter().collect();
        assert!(colors.len() > 100);
        if let Ok(path) = std::env::var("CAMERA_DECORATED_TEST_IMAGE") {
            std::fs::write(path, bytes).unwrap();
        }
    }
}

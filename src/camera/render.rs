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

    // CP9, I66: with the floor's desk hidden, a working bot sits in front of an item that serves as a
    // desk, as on the page; with none, it sits at the table.
    #[test]
    fn with_the_desk_hidden_a_working_bot_uses_a_computer_or_else_the_table() {
        let computer = json!([{"id":"c","item":"computer","x":5,"y":6,"w":2,"h":1,"facing":0}]);
        let mut scene = room(computer, json!([bot_in("w", "idle", true)]), whole());
        scene["look"]["hidden"] = json!(["desk"]);
        let at = places(&scene);
        assert_eq!(at[0].3, "sit", "{at:?}");
        assert_ne!((at[0].1, at[0].2), (44.0, 146.0), "not the hidden desk: {at:?}");
        assert_ne!((at[0].1, at[0].2), (192.0, 127.0), "not the table: {at:?}");
        assert_eq!(png(&scene).unwrap().at_desk, ["w"]);
        scene["items"] = json!([]);
        let at = places(&scene);
        assert_eq!((at[0].1, at[0].2, at[0].3.as_str()), (192.0, 127.0, "sit"), "{at:?}");
        assert_eq!(png(&scene).unwrap().at_table, ["w"]);
    }

    /// Each name tag's box as placed: [x left, y top, width, height].
    fn tags(scene: &Value) -> Vec<(f64, f64, f64, f64)> {
        let out = run(scene, Some("const s=JSON.parse(__camera_scene),g=new CameraCanvas();const p=cameraPlaced(s);JSON.stringify(placeLabels(g,cameraFigures(s,p),s.frame,s.frame.width<480?14:16).map(t=>[t.x-t.t.width/2,t.y-6,t.t.width,t.t.height]))")).unwrap();
        serde_json::from_str(&out).unwrap()
    }

    fn apart(tags: &[(f64, f64, f64, f64)]) -> bool {
        tags.iter().enumerate().all(|(i, a)| {
            tags[i + 1..].iter().all(|b| {
                a.0 + a.2 <= b.0 || b.0 + b.2 <= a.0 || a.1 + a.3 <= b.1 || b.1 + b.3 <= a.1
            })
        })
    }

    // CP11: seated bots' tags go above their heads, clear of the table they sit at; a standing
    // figure's tag stays under its feet. Still none covers another (CP10).
    #[test]
    fn seated_tags_go_above_heads_and_leave_the_table_in_view() {
        let out = |scene: &Value| -> Vec<(f64, f64, f64, f64, f64, String)> {
            let script = "const s=JSON.parse(__camera_scene),g=new CameraCanvas();const p=cameraPlaced(s);JSON.stringify(placeLabels(g,cameraFigures(s,p),s.frame,s.frame.width<480?14:16).map(t=>[t.x-t.t.width/2,t.y-6,t.t.width,t.t.height,t.f.y,t.f.pose]))";
            serde_json::from_str(&run(scene, Some(script)).unwrap()).unwrap()
        };
        let mut bots = Vec::new();
        for name in ["Grokliver Tree", "AgentsHangout PR", "morgbot 2.0", "Town Square Hermes"] {
            bots.push(bot_in(name, "active", false));
        }
        bots.push(bot_in("Standing Sam", "idle", false));
        let placed = out(&room(json!([]), json!(bots), whole()));
        assert_eq!(placed.len(), 5, "{placed:?}");
        let (seated, standing): (Vec<_>, Vec<_>) = placed.iter().partition(|t| t.5 == "sit");
        assert_eq!(seated.len(), 4, "{placed:?}");
        for t in &seated {
            assert!(t.1 + t.3 <= t.4 - 20.0, "above its head, off the table: {t:?}");
        }
        for t in &standing {
            assert!(t.1 >= t.4, "under its feet: {t:?}");
        }
        let boxes: Vec<_> = placed.iter().map(|t| (t.0, t.1, t.2, t.3)).collect();
        assert!(apart(&boxes), "{boxes:?}");
    }

    // CP10: bots side by side at the table, with long names, get name tags that don't cover each
    // other, and every tag stays in the frame. So does a crowded grid in a small frame.
    #[test]
    fn name_tags_never_cover_each_other() {
        let mut bots = Vec::new();
        for name in ["Grokliver Tree", "AgentsHangout PR", "morgbot 2.0", "Town Square Hermes"] {
            bots.push(bot_in(name, "active", false));
        }
        let inside = |tags: &[(f64, f64, f64, f64)], w: f64, h: f64| {
            tags.iter().all(|t| t.0 >= 0.0 && t.0 + t.2 <= w && t.1 >= 0.0 && t.1 + t.3 <= h)
        };
        let at_table = tags(&room(json!([]), json!(bots), whole()));
        assert_eq!(at_table.len(), 4, "everyone at the table gets a tag: {at_table:?}");
        assert!(apart(&at_table), "{at_table:?}");
        assert!(inside(&at_table, 480.0, 252.0), "{at_table:?}");

        // Too many for a small frame: tags shorten or drop, never cover each other or leave it.
        let crowd: Vec<Value> =
            (0..12).map(|n| bot(&format!("Visiting bot number {n}"), None)).collect();
        let small = json!({"x":0,"y":0,"width":240,"height":126});
        let grid = tags(&room(json!([]), json!(crowd), small));
        assert!(!grid.is_empty() && grid.len() <= 12, "{grid:?}");
        assert!(apart(&grid), "{grid:?}");
        assert!(inside(&grid, 240.0, 126.0), "{grid:?}");
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

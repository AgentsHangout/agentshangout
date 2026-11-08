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

    // CP13: a tag is snug around its name, as the font really draws it: never narrower than the
    // words, with room each side, and much narrower than the old 5.5 a letter.
    #[test]
    fn a_tag_fits_its_name_as_drawn() {
        let mut options = resvg::usvg::Options::default();
        options
            .fontdb_mut()
            .load_font_data(include_bytes!("../../assets/fonts/DejaVuSans.ttf").to_vec());
        for name in [
            "Grokliver Tree",
            "AgentsHangout PR",
            "morgbot 2.0",
            "WWWWWWWWWWWWWWWW",
            "mmmmmmmmmmmmmmmm",
            "Ω漢字 Ünïcödé",
            "x",
        ] {
            let script = format!(
                "const g=new CameraCanvas(),t=g.labelLines({},16);g.label(200,50,t);JSON.stringify([t.width,t.lines.length,g.parts.filter(p=>p.startsWith('<text')).join('')])",
                json!(name)
            );
            let (width, lines, texts): (f64, usize, String) =
                serde_json::from_str(&run(&json!({}), Some(&script)).unwrap()).unwrap();
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="100">{texts}</svg>"#
            );
            let tree = resvg::usvg::Tree::from_str(&svg, &options).unwrap();
            let drawn = tree.root().abs_bounding_box().width() as f64;
            assert!(drawn + 4.0 <= width, "{name}: drawn {drawn}, tag {width}");
            let chars = name.chars().count().min(16) as f64;
            if lines == 1 && ["Grokliver Tree", "AgentsHangout PR"].contains(&name) {
                assert!(width < chars * 5.5 + 8.0 - 10.0, "{name}: {width}");
            }
        }
        // Three bots side by side at the table need two rows of tags, not a staircase of three.
        let bots: Vec<_> = ["Cake Claude", "Grokliver Tree", "morgbot 2.0"]
            .iter()
            .map(|n| bot_in(n, "active", false))
            .collect();
        let three = tags(&room(json!([]), json!(bots), whole()));
        assert_eq!(three.len(), 3);
        let mut rows: Vec<_> = three.iter().map(|t| t.1 as i64).collect();
        rows.sort();
        rows.dedup();
        assert_eq!(rows.len(), 2, "{three:?}");
    }

    // CP14: nobody standing is drawn on the table, an item or another figure, even when their place
    // in the grid falls there: they stand on the nearest cell the page would let them walk on.
    #[test]
    fn standing_figures_keep_off_the_table_items_and_each_other() {
        let stands = |scene: &Value| -> Vec<(f64, f64, bool)> {
            let script = "const s=JSON.parse(__camera_scene);const f=cameraFigures(s,cameraPlaced(s)).filter(f=>f.pose==='stand');JSON.stringify(f.map(f=>[f.x,f.y,clear(f.x,f.y)&&clear(f.x,f.y-12)]))";
            serde_json::from_str(&run(scene, Some(script)).unwrap()).unwrap()
        };
        let piano = json!([{"id":"piano","item":"piano","x":30,"y":8,"w":4,"h":2,"facing":2}]);
        let person = |name: &str| json!({"name":name,"color":"#ff8866","look":{}});
        // One person alone: the middle of the grid is the middle of the table.
        let seated: Vec<_> =
            ["a", "b", "c", "d"].iter().map(|n| bot_in(n, "active", false)).collect();
        let mut one = room(piano.clone(), json!(seated), whole());
        one["people"] = json!([person("Justin")]);
        // A crowd, in the whole room and in the table frame.
        let idle: Vec<_> = (0..8).map(|i| bot_in(&format!("idle{i}"), "idle", false)).collect();
        let mut crowd = room(piano.clone(), json!(idle), whole());
        crowd["people"] = json!([person("Ann"), person("Bo")]);
        let mut close = crowd.clone();
        close["frame"] = json!({"x":120,"y":60,"width":240,"height":126});
        for scene in [&one, &crowd, &close] {
            let placed = stands(scene);
            assert!(!placed.is_empty());
            for (i, a) in placed.iter().enumerate() {
                assert!(a.2, "standing on furniture: {a:?} in {placed:?}");
                for b in &placed[i + 1..] {
                    assert!(
                        (a.0 - b.0).abs() >= 16.0 || (a.1 - b.1).abs() >= 12.0,
                        "{a:?} on {b:?}"
                    );
                }
            }
        }
        let justin = stands(&one)[0];
        assert!(justin.1 >= 182.0, "in front of the table, not on it: {justin:?}");
    }

    // CP11: a bot sitting with its back to the camera, on the table's near side, has the table above
    // it, so its tag goes under its feet, not onto the tabletop.
    #[test]
    fn a_back_facing_seat_puts_its_tag_under_the_feet() {
        let script = "const s=JSON.parse(__camera_scene),g=new CameraCanvas();const p=cameraPlaced(s);JSON.stringify(placeLabels(g,cameraFigures(s,p),s.frame,s.frame.width<480?14:16).map(t=>[t.f.name,t.y-6,t.f.y,t.f.facing]))";
        let bots: Vec<_> =
            ["a1", "a2", "a3", "a4", "near"].iter().map(|n| bot_in(n, "active", false)).collect();
        let placed: Vec<(String, f64, f64, String)> = serde_json::from_str(
            &run(&room(json!([]), json!(bots), whole()), Some(script)).unwrap(),
        )
        .unwrap();
        let near = placed.iter().find(|t| t.0 == "near").expect("the fifth bot sits too");
        assert_eq!(near.3, "back", "{placed:?}");
        assert!(near.1 >= near.2, "under its feet, off the table: {near:?}");
        for far in placed.iter().filter(|t| t.3 == "front") {
            assert!(far.1 < far.2, "a far-side seat's tag stays above its head: {far:?}");
        }
    }

    // CP18: a full table of long names keeps its tags off the wall board; one with no room above
    // goes under its feet.
    #[test]
    fn no_tag_covers_the_wall_board() {
        let bots: Vec<_> = ["Cake Claude", "Grokliver Tree", "Ribbit Red Velvet", "Dynamic Dan"]
            .iter()
            .map(|n| bot_in(n, "active", false))
            .collect();
        let placed = tags(&room(json!([]), json!(bots), whole()));
        assert_eq!(placed.len(), 4, "{placed:?}");
        assert!(apart(&placed), "{placed:?}");
        for t in &placed {
            let off = t.0 + t.2 <= 146.0 || t.0 >= 334.0 || t.1 >= 58.0;
            assert!(off, "covers the board: {t:?}");
        }
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
    // CP12
    #[test]
    fn the_board_shows_the_room_name_cut_to_one_line_and_nothing_else() {
        let svg = |title: Value| {
            let mut scene = room(json!([]), json!([]), whole());
            scene["title"] = title;
            scene["whiteboard"] = json!("secret plan");
            let out: Value = serde_json::from_str(&run(&scene, None).unwrap()).unwrap();
            out["svg"].as_str().unwrap().to_string()
        };
        let board = |svg: &str| svg.matches("font-size=\"11\"").count();
        let long = svg(json!("Commons <Fair> & a very long name"));
        assert!(long.contains(">Commons &lt;Fair&gt; &amp; a very l…</text>"), "{long}");
        assert_eq!(board(&long), 1);
        assert!(!long.contains("secret"));
        assert!(svg(json!("Lab")).contains(">Lab</text>"));
        assert_eq!(board(&svg(json!("  "))), 0);
        assert_eq!(board(&svg(Value::Null)), 0);
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

    /// The Commons Fairground's items as the live room has them: kind, x, y, facing.
    const FAIRGROUND: [(&str, i32, i32, i32); 36] = [
        ("flower_box", 32, 0, 0),
        ("flower_box", 36, 0, 0),
        ("hedge", 13, 0, 0),
        ("hedge", 17, 0, 0),
        ("hedge", 21, 0, 0),
        ("hedge", 25, 0, 0),
        ("bench", 2, 8, 0),
        ("lantern", 0, 8, 0),
        ("lantern", 7, 8, 0),
        ("bench", 32, 6, 0),
        ("boulder", 29, 6, 0),
        ("flower_box", 0, 12, 0),
        ("flower_box", 28, 12, 0),
        ("rug_checked", 33, 2, 0),
        ("standing_desk", 34, 3, 0),
        ("whiteboard", 32, 1, 0),
        ("computer", 38, 2, 0),
        ("rubber_duck", 38, 4, 0),
        ("stool", 35, 5, 0),
        ("monitor", 30, 3, 0),
        ("stool", 30, 4, 0),
        ("bookshelf", 2, 1, 0),
        ("globe", 6, 1, 0),
        ("coffee_table", 3, 2, 0),
        ("stool", 2, 2, 0),
        ("torch", 1, 1, 0),
        ("stool", 6, 2, 0),
        ("plant", 1, 3, 0),
        ("chest", 2, 4, 0),
        ("piano", 20, 10, 0),
        ("guitar", 19, 11, 0),
        ("whiteboard", 11, 11, 0),
        ("easel", 36, 9, 0),
        ("microscope", 29, 0, 0),
        ("corkboard", 4, 11, 0),
        ("lific_board", 28, 9, 0),
    ];

    fn fairground(bots: Value) -> Value {
        let items: Vec<_> = FAIRGROUND
            .iter()
            .enumerate()
            .map(|(n, (kind, x, y, facing))| {
                let k = crate::items::kind_of(kind);
                json!({"id":format!("i{n}"),"item":kind,"x":x,"y":y,"w":k.map_or(1, |k| k.w),"h":k.map_or(1, |k| k.h),"facing":facing})
            })
            .collect();
        let mut scene = room(json!(items), bots, whole());
        // The live Fairground's look: outdoors on grass, the default furniture hidden, a picnic table.
        scene["look"] = json!({"paint":"sage","flooring":"grass","outdoors":true,
            "hidden":["bar","arcade","plants","rug","beanbags","couch","desk"],
            "table":{"x":12,"y":1,"style":"picnic"}});
        scene
    }

    // CP19: a lone bot standing in the Fairground stands where its name tag, under its feet, covers
    // no whiteboard, guitar or piano, alone and with someone seated at the table.
    #[test]
    fn a_standing_bot_stands_where_its_tag_covers_no_item() {
        let rects = |scene: &Value| -> Vec<(f64, f64, f64, f64)> {
            let script = "const s=JSON.parse(__camera_scene);const [dx,dy]=tableShift(s.look.table),t=FURNITURE.find(e=>e[0]==='table');JSON.stringify([...cameraPlaced(s).filter(i=>!i.flat).map(i=>{const up=Math.max(i.rise||0,i.deep);return [i.left,i.y-up,i.wide,up]}),[t[1]+dx,t[2]+dy-12,t[3],t[4]+12]])";
            serde_json::from_str(&run(scene, Some(script)).unwrap()).unwrap()
        };
        let lone = bot_in("Town Square Hermes", "idle", false);
        let seated = bot_in("Cake Claude", "active", false);
        for bots in [json!([lone]), json!([seated, lone])] {
            let scene = fairground(bots);
            let placed = tags(&scene);
            assert_eq!(placed.len(), scene["bots"].as_array().unwrap().len(), "{placed:?}");
            assert!(apart(&placed), "{placed:?}");
            for t in &placed {
                assert!(
                    t.0 >= 0.0 && t.0 + t.2 <= 480.0 && t.1 >= 0.0 && t.1 + t.3 <= 252.0,
                    "{t:?}"
                );
            }
            let hermes = placed.last().unwrap();
            for r in rects(&scene) {
                let over = hermes.0 < r.0 + r.2
                    && r.0 < hermes.0 + hermes.2
                    && hermes.1 < r.1 + r.3
                    && r.1 < hermes.1 + hermes.3;
                assert!(!over, "tag {hermes:?} covers item {r:?}");
            }
        }
    }
}

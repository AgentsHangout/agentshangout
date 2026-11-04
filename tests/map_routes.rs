//! Tests for how a figure on the map gets from one place to the next. They run the map's own route
//! code from web/world.js in Node, and are written from L61 in docs/routines.md and the city's map
//! in docs/city.md. A comment such as `// L61` above a test names the rule it covers.

use std::process::Command;

/// Runs `script` after the map's route code, with `world` standing in for the map.
fn run(script: &str) -> String {
    let source = include_str!("../web/world.js").replace("\r\n", "\n");
    let method = |name: &str| {
        let start = source.find(&format!("\n  {name}(")).unwrap_or_else(|| panic!("no {name}"));
        let end = start + source[start..].find("\n  }\n").unwrap() + 4;
        source[start..end].to_string()
    };
    let constant = |name: &str| {
        let start =
            source.find(&format!("\nconst {name} = ")).unwrap_or_else(|| panic!("no {name}"));
        let end = start + source[start..].find(";\n").unwrap() + 2;
        source[start..end].to_string()
    };
    let mut program = String::new();
    for name in [
        "GRASS_L",
        "FOREST_L",
        "WATER_L",
        "HILL_L",
        "K_ROAD",
        "K_CLEARED",
        "K_HOMES",
        "K_WORKS",
        "DRIVE_TILES",
        "DIRS4",
    ] {
        program += &constant(name);
    }
    program += "const isZone = (k) => k >= K_HOMES && k <= K_WORKS;\n";
    program += "class World {\n";
    for name in ["walkable", "findPath", "roadAt", "routeTo"] {
        program += &method(name);
    }
    program += "\n  sparkle() { this.sparkles += 1; }\n}\n";
    program += "const clamp = (v, a, b) => Math.max(a, Math.min(b, v));\n";
    program += "const ih = () => 0;\n";
    program += script;
    let out = Command::new("node").arg("-e").arg(&program).output().expect("node runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

// L61: a bot drives to a place across the whole map, however far it is, instead of vanishing.
#[test]
fn a_bot_drives_all_the_way_across_the_map() {
    let out = run("
        const n = 128;
        const world = Object.assign(new World(), {
          n, calm: false, sparkles: 0,
          land: new Uint8Array(n * n).fill(GRASS_L),
          kind: new Uint8Array(n * n),
          level: new Uint8Array(n * n),
        });
        // A road along the top and down the right, from one corner of the map to the other.
        for (let x = 0; x < n; x++) world.kind[x] = K_ROAD;
        for (let y = 0; y < n; y++) world.kind[y * n + n - 1] = K_ROAD;
        // Forest everywhere else, so the road is the only way.
        for (let i = 0; i < n * n; i++) if (world.kind[i] !== K_ROAD) world.land[i] = FOREST_L;
        const bot = { px: 0.5, py: 0.5, ox: 0, oy: 0, h: 1, path: [] };
        world.routeTo(bot, n - 1, n - 1);
        console.log(JSON.stringify({
          sparkles: world.sparkles,
          steps: bot.path.length,
          driven: bot.path.filter((step) => step[2]).length,
          ride: Boolean(bot.ride),
        }));
    ");
    assert_eq!(
        out, r#"{"sparkles":0,"steps":254,"driven":254,"ride":true}"#,
        "it should drive the whole road, not vanish"
    );
}

// L61, and the city's map: with no way there at all, it still vanishes and appears at the other end.
#[test]
fn a_bot_with_no_way_there_vanishes_and_appears() {
    let out = run("
        const n = 128;
        const world = Object.assign(new World(), {
          n, calm: false, sparkles: 0,
          land: new Uint8Array(n * n).fill(GRASS_L),
          kind: new Uint8Array(n * n),
          level: new Uint8Array(n * n),
        });
        // A river all the way down the middle.
        for (let y = 0; y < n; y++) world.land[y * n + 64] = WATER_L;
        const bot = { px: 10.5, py: 10.5, ox: 0, oy: 0, h: 1, path: [] };
        world.routeTo(bot, 120, 100);
        console.log(JSON.stringify({ sparkles: world.sparkles, steps: bot.path.length, at: [bot.px, bot.py] }));
    ");
    assert_eq!(out, r#"{"sparkles":2,"steps":0,"at":[120.5,100.5]}"#);
}

// L61: a route that crosses grass, where each step costs 1.7, is found like one on the road. The
// costs were kept in a Float32Array: 1.7 rounds there, so a step's stored cost could come out a hair
// under its real one and the search skipped it as stale. Most routes across grass then failed, and
// bots vanished and appeared at the other end instead of driving.
#[test]
fn a_bot_walks_over_grass_to_the_road_and_drives_on() {
    let out = run("
        const n = 128;
        const world = Object.assign(new World(), {
          n, calm: false, sparkles: 0,
          land: new Uint8Array(n * n).fill(GRASS_L),
          kind: new Uint8Array(n * n),
          level: new Uint8Array(n * n),
        });
        // Open grass, with a road across at y 64 and down at x 59, as near The Commons' town square.
        for (let x = 0; x < n; x++) world.kind[64 * n + x] = K_ROAD;
        for (let y = 64; y < n; y++) world.kind[y * n + 59] = K_ROAD;
        const across = { px: 51.5, py: 62.5, ox: 0, oy: 0, h: 1, path: [] };
        world.routeTo(across, 55, 62);
        const there = { px: 53.5, py: 62.5, ox: 0, oy: 0, h: 1, path: [] };
        world.routeTo(there, 61, 81);
        console.log(JSON.stringify({
          sparkles: world.sparkles,
          across: across.path.length,
          there: there.path.length,
          ride: Boolean(there.ride),
        }));
    ");
    assert_eq!(
        out, r#"{"sparkles":0,"across":4,"there":27,"ride":true}"#,
        "it should walk to the road and drive, not vanish"
    );
}

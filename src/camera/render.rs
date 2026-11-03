use serde_json::Value;

pub fn png(scene: &Value) -> Result<Vec<u8>, String> {
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
    let script = format!("{art}\n{}", include_str!("../../web/camera-render.js"));
    let svg: String = context
        .with(|ctx| {
            ctx.globals().set("__camera_scene", scene.to_string())?;
            ctx.eval::<String, _>(script)
        })
        .map_err(|e| format!("camera drawing failed: {e}"))?;
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
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn room_art_renders_as_a_bounded_png() {
        let image = png(&serde_json::json!({
            "look":{"paint":"dusk","flooring":"oak","hidden":[],"outdoors":false,"table":null},
            "items":[], "bots":[{"id":"bot","name":"Dotliver Tree","color":"#8866cc","look":{}}],
            "frame":{"x":0,"y":0,"width":480,"height":252}
        }))
        .unwrap();
        assert_eq!(&image[..8], b"\x89PNG\r\n\x1a\n");
        assert!(image.len() < 512 * 1024);
    }
    #[test]
    fn decorated_outdoor_room_and_xml_like_names_render_without_external_resources() {
        let scene = serde_json::json!({
            "look":{"paint":"sage","flooring":"grass","hidden":["bar"],"outdoors":true,"table":{"x":10,"y":1,"style":"round"}},
            "items":[{"id":"table","item":"coffee_table","x":3,"y":7,"w":3,"h":2,"facing":1},
                     {"id":"piano","item":"piano","x":30,"y":8,"w":4,"h":2,"facing":2}],
            "bots":[{"id":"one","name":"Bot\u{1} <&\" Ω","color":"#8866cc","look":{}},
                    {"id":"two","name":"morgbot 2.0","color":"#44aaff","look":{}},
                    {"id":"three","name":"AgentsHangout PR","color":"#ff8866","look":{}}],
            "frame":{"x":80,"y":60,"width":320,"height":168}
        });
        let bytes = png(&scene).unwrap();
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

//! #927 review: every module app.js imports (static or dynamic) is served as JavaScript, so a missing one can't take the page down.
use agenthangout::{app, db::Db, AppState, Config};
use reqwest::{Client, StatusCode};

// The "./name.js" specifiers that app.js imports: `from './x.js'`, `import './x.js'` and `import('./x.js')`.
fn imported_modules(source: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for quote in ['\'', '"'] {
        let marker = format!("{quote}./");
        for (start, _) in source.match_indices(&marker) {
            let before = source[..start].trim_end();
            let imports = before.ends_with("from")
                || before.ends_with("import")
                || before.ends_with("import(");
            if !imports {
                continue;
            }
            let name = source[start + 3..].split(quote).next().unwrap_or("");
            if name.ends_with(".js") && !name.contains('/') && !found.iter().any(|f| f == name) {
                found.push(name.to_string());
            }
        }
    }
    found
}

#[tokio::test]
async fn every_module_app_js_imports_is_served() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let state = AppState::new(
        Db::open(":memory:").unwrap(),
        Config {
            public_url: base.clone(),
            create_code: None,
            allow_private_webhooks: true,
            keeper_id: None,
            god_ids: vec![],
            require_rules: false,
        },
    );
    let task = tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });
    let http = Client::new();

    let page = http.get(format!("{base}/app.js")).send().await.unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let source = page.text().await.unwrap();

    let modules = imported_modules(&source);
    // app.js imports eight modules statically, plus the lazy ones. If this finds fewer, the parser is wrong.
    assert!(modules.len() >= 8, "found only {modules:?}");
    assert!(modules.iter().any(|m| m == "bug-report.js"), "{modules:?}");
    assert!(modules.iter().any(|m| m == "room.js"), "{modules:?}");

    for name in &modules {
        let response = http.get(format!("{base}/{name}")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "/{name} must be served");
        let content_type = response.headers()["content-type"].to_str().unwrap().to_string();
        assert!(content_type.starts_with("text/javascript"), "/{name} is {content_type}");
        assert!(!response.bytes().await.unwrap().is_empty(), "/{name} is empty");
    }

    // The page itself loads app.js from its versioned client folder, and the browser resolves
    // every import there, so follow the whole module graph from app.js in that folder. Root paths
    // alone passed while /client/<version>/bug-report.js was a 404 that rolled back v0.1.580.
    let home = http.get(format!("{base}/")).send().await.unwrap().text().await.unwrap();
    let preload = "<link rel=\"modulepreload\" href=\"";
    let at = home.find(preload).expect("the page preloads app.js") + preload.len();
    let app_url = &home[at..at + home[at..].find('"').unwrap()];
    let dir = app_url.strip_suffix("/app.js").expect("the page preloads app.js");
    assert!(dir.starts_with("/client/"), "{dir}");
    let mut graph = vec!["app.js".to_string()];
    let mut next = 0;
    while next < graph.len() {
        let name = graph[next].clone();
        next += 1;
        let response = http.get(format!("{base}{dir}/{name}")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{dir}/{name} must be served");
        let content_type = response.headers()["content-type"].to_str().unwrap().to_string();
        assert!(content_type.starts_with("text/javascript"), "{dir}/{name} is {content_type}");
        let source = response.text().await.unwrap();
        assert!(!source.is_empty(), "{dir}/{name} is empty");
        for found in imported_modules(&source) {
            if !graph.contains(&found) {
                graph.push(found);
            }
        }
    }
    assert!(graph.iter().any(|m| m == "bug-report.js"), "{graph:?}");
    task.abort();
}

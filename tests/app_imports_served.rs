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
            let imports = before.ends_with("from") || before.ends_with("import") || before.ends_with("import(");
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
    task.abort();
}

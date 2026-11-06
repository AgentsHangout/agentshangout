//! M6–M7: every platform loads one immutable version of the shared client.
use agenthangout::{app, db::Db, AppState, Config};
use reqwest::{Client, StatusCode};
use sha2::{Digest, Sha256};

#[test]
fn worker_update_is_atomic_and_keeps_open_versions() {
    let output = std::process::Command::new("node")
        .arg("tests/client_worker.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn shared_client_assets_are_complete_immutable_and_never_substituted() {
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
        },
    );
    let task = tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });
    let http = Client::new();
    let response = http.get(format!("{base}/client-manifest.json")).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-cache");
    let manifest: serde_json::Value = response.json().await.unwrap();
    let version = manifest["version"].as_str().unwrap();
    assert_eq!(version.len(), 64);
    let files = manifest["files"].as_array().unwrap();
    for file in files {
        let path = file["url"].as_str().unwrap();
        assert!(path.starts_with(&format!("/client/{version}/")));
        let response = http.get(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(response.headers()["cache-control"], "public, max-age=31536000, immutable");
        let bytes = response.bytes().await.unwrap();
        assert_eq!(
            Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect::<String>(),
            file["sha256"]
        );
        if path.ends_with(".js") {
            let source = std::str::from_utf8(&bytes).unwrap();
            for part in source.split("from './").skip(1) {
                let name = part.split('\'').next().unwrap();
                assert!(
                    files.iter().any(|f| f["url"] == format!("/client/{version}/{name}")),
                    "missing {name}"
                );
            }
        }
    }
    for path in ["/", "/worlds", "/h/test", "/setup/test"] {
        let page = http.get(format!("{base}{path}")).send().await.unwrap().text().await.unwrap();
        assert!(page.contains(&format!("src=\"/client/{version}/client-entry.js\"")), "{path}");
        assert!(page.contains(&format!("href=\"/client/{version}/style.css\"")), "{path}");
    }
    let wrong = "0".repeat(64);
    for path in [format!("/client/{wrong}/app.js"), format!("/client/{version}/private.env")] {
        let response = http.get(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    let worker = http.get(format!("{base}/client-worker.js")).send().await.unwrap();
    assert_eq!(worker.headers()["cache-control"], "no-cache");
    assert!(worker.text().await.unwrap().contains(version));
    task.abort();
}

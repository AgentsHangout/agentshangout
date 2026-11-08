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
            require_rules: false,
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

// W118
#[tokio::test]
async fn answers_are_gzipped_only_for_a_client_that_asks() {
    use std::io::Read;
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
    let manifest: serde_json::Value = http
        .get(format!("{base}/client-manifest.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let version = manifest["version"].as_str().unwrap();
    for path in [format!("/client/{version}/app.js"), "/".into(), "/api/worlds/commons".into()] {
        let plain = http.get(format!("{base}{path}")).send().await.unwrap();
        assert!(plain.headers().get("content-encoding").is_none(), "{path}");
        let plain = plain.bytes().await.unwrap();
        let zipped = http
            .get(format!("{base}{path}"))
            .header("accept-encoding", "gzip")
            .send()
            .await
            .unwrap();
        assert_eq!(zipped.headers()["content-encoding"], "gzip", "{path}");
        let zipped = zipped.bytes().await.unwrap();
        assert!(zipped.len() < plain.len(), "{path}");
        let mut unzipped = Vec::new();
        flate2::read::GzDecoder::new(&zipped[..]).read_to_end(&mut unzipped).unwrap();
        assert_eq!(unzipped, plain, "{path}");
    }
    for path in ["/agent-plugin.zip", "/og.jpg"] {
        let response = http
            .get(format!("{base}{path}"))
            .header("accept-encoding", "gzip")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert!(response.headers().get("content-encoding").is_none(), "{path}");
    }
    // A signed-in answer goes out as it is, but the client files stay gzipped.
    let signed_in = [
        ("/api/worlds/commons".to_string(), "authorization", "Bearer x", None),
        ("/".to_string(), "cookie", "session=x", None),
        (format!("/client/{version}/app.js"), "cookie", "session=x", Some("gzip")),
    ];
    for (path, name, value, want) in signed_in {
        let response = http
            .get(format!("{base}{path}"))
            .header("accept-encoding", "gzip")
            .header(name, value)
            .send()
            .await
            .unwrap();
        let got = response.headers().get("content-encoding").map(|v| v.to_str().unwrap());
        assert_eq!(got, want, "{path} with {name}");
    }
    task.abort();
}

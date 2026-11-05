//! #552, L62: bots drive to and from places on the map. A test server gives the real Commons map,
//! and tests/drive_ui.mjs runs the shipped web/world.js on it.
use agenthangout::{app, db::Db, AppState, Config};

#[tokio::test]
async fn bots_drive_to_and_from_places() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let state = AppState::new(
        Db::open(":memory:").unwrap(),
        Config {
            public_url: base.clone(),
            create_code: Some("test".into()),
            allow_private_webhooks: true,
            keeper_id: None,
            god_ids: vec![],
        },
    );
    let server = tokio::spawn(async move { axum::serve(listener, app(state)).await });
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("node")
            .args(["tests/drive_ui.mjs", &base])
            .output()
            .expect("node runs")
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}
{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    server.abort();
}

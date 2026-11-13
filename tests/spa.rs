use axum_test::TestServer;

fn dist_is_empty() -> bool {
    if upland::spa_enabled() {
        false
    } else {
        eprintln!("SKIPPING: web/dist is empty; build the frontend to run SPA tests");
        true
    }
}

#[tokio::test]
async fn visitor_receives_spa_index_at_root() {
    if dist_is_empty() {
        return;
    }
    let server = TestServer::new(upland::router());
    let response = server.get("/").await;

    response.assert_status_ok();
    let body = response.text();
    assert!(body.contains(r#"<div id="root">"#), "index html expected");
    assert!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/html"))
    );
}

#[tokio::test]
async fn visitor_can_deep_link_to_any_spa_route() {
    if dist_is_empty() {
        return;
    }
    let server = TestServer::new(upland::router());
    let response = server.get("/some/spa/route").await;

    response.assert_status_ok();
    assert!(response.text().contains(r#"<div id="root">"#));
}

#[tokio::test]
async fn bundled_asset_is_served_with_its_bytes() {
    if dist_is_empty() {
        return;
    }
    let assets_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("web/dist/assets");
    let asset = std::fs::read_dir(assets_dir)
        .expect("web/dist/assets should exist when dist is built")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "js"))
        .expect("at least one bundled js asset");
    let file_name = asset.file_name().unwrap().to_str().unwrap().to_owned();
    let expected_bytes = std::fs::read(&asset).unwrap();

    let server = TestServer::new(upland::router());
    let url = format!("/assets/{file_name}");
    let response = server.get(url.as_str()).await;

    response.assert_status_ok();
    assert_eq!(response.as_bytes(), &expected_bytes);
}

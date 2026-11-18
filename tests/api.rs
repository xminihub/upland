use axum::http::StatusCode;
use axum_test::TestServer;
use serde_json::{Value, json};

/// Every HTTP test gets the production shape: a router holding a real
/// `AppState` over an in-memory database (ADR-0003). No test opens a socket;
/// SQLite is a file library and the in-memory variant touches neither.
async fn test_server() -> TestServer {
    let db = upland::db::Db::in_memory()
        .await
        .expect("in-memory test database");
    TestServer::new(upland::router(upland::AppState {
        db: std::sync::Arc::new(db),
    }))
}

#[tokio::test]
async fn api_consumer_receives_hello_at_api_prefix() {
    let server = test_server().await;
    let response = server.get("/api/hello").await;

    response.assert_status_ok();
    assert_eq!(
        response.json::<Value>(),
        json!({ "message": "Hello from upland!" })
    );
}

#[tokio::test]
async fn api_consumer_gets_json_not_found_for_unknown_api_path() {
    let server = test_server().await;
    let response = server.get("/api/nope").await;

    response.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(response.json::<Value>(), json!({ "error": "not found" }));
}

#[tokio::test]
async fn operator_can_check_health() {
    let server = test_server().await;
    let response = server.get("/healthz").await;

    response.assert_status_ok();
    assert_eq!(response.json::<Value>(), json!({ "status": "ok" }));
}

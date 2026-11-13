use axum::http::StatusCode;
use axum_test::TestServer;
use serde_json::{Value, json};

#[tokio::test]
async fn api_consumer_receives_hello_at_api_prefix() {
    let server = TestServer::new(upland::router());
    let response = server.get("/api/hello").await;

    response.assert_status_ok();
    assert_eq!(
        response.json::<Value>(),
        json!({ "message": "Hello from upland!" })
    );
}

#[tokio::test]
async fn api_consumer_gets_json_not_found_for_unknown_api_path() {
    let server = TestServer::new(upland::router());
    let response = server.get("/api/nope").await;

    response.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(response.json::<Value>(), json!({ "error": "not found" }));
}

#[tokio::test]
async fn operator_can_check_health() {
    let server = TestServer::new(upland::router());
    let response = server.get("/healthz").await;

    response.assert_status_ok();
    assert_eq!(response.json::<Value>(), json!({ "status": "ok" }));
}

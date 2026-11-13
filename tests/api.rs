use axum_test::TestServer;
use serde_json::{Value, json};

#[tokio::test]
async fn visitor_receives_hello_at_root() {
    let server = TestServer::new(upland::router());
    let response = server.get("/").await;

    response.assert_status_ok();
    assert_eq!(
        response.json::<Value>(),
        json!({ "message": "Hello from upland!" })
    );
}

#[tokio::test]
async fn operator_can_check_health() {
    let server = TestServer::new(upland::router());
    let response = server.get("/healthz").await;

    response.assert_status_ok();
    assert_eq!(response.json::<Value>(), json!({ "status": "ok" }));
}

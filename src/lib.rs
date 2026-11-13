use axum::Json;
use axum::Router;
use axum::routing::get;
use serde_json::{Value, json};

pub fn router() -> Router {
    Router::new()
        .route("/", get(hello))
        .route("/healthz", get(healthz))
}

async fn hello() -> Json<Value> {
    Json(json!({ "message": "Hello from upland!" }))
}

async fn healthz() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

#[cfg(test)]
mod tests {
    use super::router;
    use axum::http::StatusCode;
    use axum_test::TestServer;

    #[tokio::test]
    async fn router_constructs_with_only_the_known_routes() {
        let server = TestServer::new(router());
        server
            .get("/nope")
            .await
            .assert_status(StatusCode::NOT_FOUND);
    }
}

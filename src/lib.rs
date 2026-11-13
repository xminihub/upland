use axum::Json;
use axum::Router;
use axum::http::StatusCode;
use axum::http::Uri;
use axum::http::header::CONTENT_TYPE;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use rust_embed::RustEmbed;
use serde_json::{Value, json};

#[derive(RustEmbed)]
#[folder = "web/dist"]
pub struct Assets;

pub fn spa_enabled() -> bool {
    Assets::iter().next().is_some()
}

pub fn router() -> Router {
    let router = Router::new()
        .nest("/api", api())
        .route("/healthz", get(healthz));
    if spa_enabled() {
        router.fallback(spa_fallback)
    } else {
        router
    }
}

async fn spa_fallback(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    match Assets::get(path) {
        Some(file) => ([(CONTENT_TYPE, file.metadata.mimetype())], file.data).into_response(),
        None => match Assets::get("index.html") {
            Some(file) => Html(file.data).into_response(),
            None => json_not_found().into_response(),
        },
    }
}

fn api() -> Router {
    Router::new()
        .route("/hello", get(hello))
        .fallback(api_not_found)
}

async fn api_not_found() -> (StatusCode, Json<Value>) {
    json_not_found()
}

fn json_not_found() -> (StatusCode, Json<Value>) {
    (StatusCode::NOT_FOUND, Json(json!({ "error": "not found" })))
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
    async fn unknown_paths_are_not_found_without_spa() {
        if crate::spa_enabled() {
            eprintln!("SKIPPING: web/dist is built; the SPA fallback owns unknown paths");
            return;
        }
        let server = TestServer::new(router());
        server
            .get("/nope")
            .await
            .assert_status(StatusCode::NOT_FOUND);
    }
}

#[tokio::main]
async fn main() {
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("listener should bind");
    axum::serve(listener, upland::router())
        .await
        .expect("server should run");
}

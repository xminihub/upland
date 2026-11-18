use std::collections::HashMap;

use axum::http::StatusCode;
use axum_test::{TestResponse, TestServer};
use serde_json::{Value, json};

use upland::OidcProvider;
use upland::oidc::{IdTokenValidator, VerifiedIdentity};

fn provider(name: &str, issuer: &str, client_id: &str) -> OidcProvider {
    OidcProvider {
        name: name.to_owned(),
        issuer: issuer.to_owned(),
        client_id: client_id.to_owned(),
    }
}

fn fixture(tokens: &[(&str, &str, &str)]) -> HashMap<String, HashMap<String, VerifiedIdentity>> {
    // tokens: (provider, id_token, email)
    let mut by_provider: HashMap<String, HashMap<String, VerifiedIdentity>> = HashMap::new();
    for (name, token, email) in tokens {
        by_provider.entry((*name).to_owned()).or_default().insert(
            (*token).to_owned(),
            VerifiedIdentity {
                subject: format!("subject-of-{token}"),
                email: (*email != "!").then(|| (*email).to_owned()),
            },
        );
    }
    by_provider
}

async fn server_with(providers: Vec<OidcProvider>, tokens: &[(&str, &str, &str)]) -> TestServer {
    let db = upland::db::Db::in_memory()
        .await
        .expect("in-memory test database");
    TestServer::new(upland::router(upland::AppState {
        db: std::sync::Arc::new(db),
        access_tokens: upland::auth::AccessTokenIssuer::new(
            upland::auth::SigningSecret::from_hex(
                "abababababababababababababababababababababababababababababababab",
            )
            .expect("test secret"),
            3600,
        ),
        oidc_providers: providers,
        id_tokens: IdTokenValidator::Fixed(fixture(tokens)),
    }))
}

async fn signup(server: &TestServer, username: &str, email: &str) -> TestResponse {
    server
        .post("/api/v1/auth/signup")
        .json(&json!({ "username": username, "email": email, "password": "hunter2" }))
        .await
}

async fn oidc_login(server: &TestServer, provider: &str, id_token: &str) -> TestResponse {
    server
        .post("/api/v1/auth/oidc")
        .json(&json!({ "provider": provider, "id_token": id_token }))
        .await
}

async fn me(server: &TestServer, token: &str) -> Value {
    server
        .get("/api/v1/auth/me")
        .add_header("authorization", format!("Bearer {token}"))
        .await
        .json::<Value>()
}

#[tokio::test]
async fn the_provider_listing_is_readable_before_anyone_signs_in() {
    let server = server_with(
        vec![
            provider("work", "https://id.example", "upland-work"),
            provider("home", "https://accounts.example", "upland-home"),
        ],
        &[],
    )
    .await;
    let response = server.get("/api/v1/auth/providers").await;
    response.assert_status_ok();
    assert_eq!(
        response.json::<Value>(),
        json!([
            { "name": "work", "issuer": "https://id.example", "client_id": "upland-work" },
            { "name": "home", "issuer": "https://accounts.example", "client_id": "upland-home" },
        ])
    );
}

#[tokio::test]
async fn an_unconfigured_server_lists_no_providers() {
    let server = server_with(vec![], &[]).await;
    let response = server.get("/api/v1/auth/providers").await;
    response.assert_status_ok();
    assert_eq!(response.json::<Value>(), json!([]));
}

#[tokio::test]
async fn an_unknown_provider_is_named_and_rejected() {
    let server = server_with(vec![provider("work", "https://id.example", "cid")], &[]).await;
    let response = oidc_login(&server, "other", "tok").await;
    response.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(
        response.json::<Value>(),
        json!({ "error": { "code": "provider_unknown", "field": "provider" } })
    );
}

#[tokio::test]
async fn a_token_the_issuer_never_signed_gets_nothing() {
    let server = server_with(
        vec![provider("work", "https://id.example", "cid")],
        &[("work", "real-token", "ada@example.com")],
    )
    .await;
    let response = oidc_login(&server, "work", "forged-token").await;
    response.assert_status_unauthorized();
    assert_eq!(
        response.json::<Value>(),
        json!({ "error": { "code": "oidc_rejected" } })
    );
}

#[tokio::test]
async fn a_novel_email_auto_creates_an_account_with_a_derived_username() {
    let server = server_with(
        vec![provider("work", "https://id.example", "cid")],
        &[("work", "ada-token", "Ada.Lovelace@Example.com")],
    )
    .await;
    let response = oidc_login(&server, "work", "ada-token").await;
    response.assert_status_ok();
    let token = response.json::<Value>()["access_token"]
        .as_str()
        .expect("token")
        .to_owned();

    let me = me(&server, &token).await;
    assert_eq!(me["username"], "ada.lovelace");
    assert_eq!(me["email"], "ada.lovelace@example.com");
    assert_eq!(me["identities"], json!(["work"]));
}

#[tokio::test]
async fn the_same_identity_signs_into_the_same_account_twice() {
    let server = server_with(
        vec![provider("work", "https://id.example", "cid")],
        &[("work", "ada-token", "ada@example.com")],
    )
    .await;
    let first = oidc_login(&server, "work", "ada-token").await;
    let second = oidc_login(&server, "work", "ada-token").await;
    assert_eq!(
        me(
            &server,
            first.json::<Value>()["access_token"].as_str().unwrap()
        )
        .await["id"],
        me(
            &server,
            second.json::<Value>()["access_token"].as_str().unwrap()
        )
        .await["id"],
    );
}

#[tokio::test]
async fn a_matching_email_links_the_identity_to_the_password_account() {
    // ADR-0004: email_verified is not consulted — the fixture says nothing
    // about it, and the link holds anyway.
    let server = server_with(
        vec![provider("work", "https://id.example", "cid")],
        &[("work", "link-token", "Grace@Example.com")],
    )
    .await;
    let signup = signup(&server, "grace", "grace@example.com").await;
    let password_token = signup.json::<Value>()["access_token"]
        .as_str()
        .unwrap()
        .to_owned();

    let oidc = oidc_login(&server, "work", "link-token").await;
    oidc.assert_status_ok();
    let oidc_token = oidc.json::<Value>()["access_token"]
        .as_str()
        .unwrap()
        .to_owned();

    let by_password = me(&server, &password_token).await;
    let by_oidc = me(&server, &oidc_token).await;
    assert_eq!(by_oidc["id"], by_password["id"]);
    assert_eq!(by_oidc["username"], "grace");
    assert_eq!(by_oidc["identities"], json!(["work"]));
}

#[tokio::test]
async fn a_derived_username_collision_gets_a_suffix() {
    let server = server_with(
        vec![provider("work", "https://id.example", "cid")],
        &[
            ("work", "first-token", "grace@example.com"),
            ("work", "second-token", "grace@other.example"),
        ],
    )
    .await;
    // "grace@example.com" auto-creates username "grace"; the second
    // identity's derived name is taken by a different email.
    oidc_login(&server, "work", "first-token").await;
    let second = oidc_login(&server, "work", "second-token").await;
    second.assert_status_ok();
    let body = second.json::<Value>();
    let token = body["access_token"].as_str().unwrap();
    assert_eq!(me(&server, token).await["username"], "grace-2");
}

#[tokio::test]
async fn a_provider_that_asserts_no_email_buys_no_account() {
    let server = server_with(
        vec![provider("work", "https://id.example", "cid")],
        &[("work", "nameless-token", "!")],
    )
    .await;
    let response = oidc_login(&server, "work", "nameless-token").await;
    response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response.json::<Value>(),
        json!({ "error": { "code": "provider_no_email" } })
    );
}

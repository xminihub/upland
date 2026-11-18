use axum::http::StatusCode;
use axum_test::{TestResponse, TestServer};
use serde_json::{Value, json};

/// Every HTTP test gets the production shape: a router holding a real
/// `AppState` over an in-memory database (ADR-0003). No test opens a socket;
/// SQLite is a file library and the in-memory variant touches neither.
async fn test_state() -> upland::AppState {
    let db = upland::db::Db::in_memory()
        .await
        .expect("in-memory test database");
    upland::AppState {
        db: std::sync::Arc::new(db),
        access_tokens: upland::auth::AccessTokenIssuer::new(test_secret(), 3600),
        oidc_providers: vec![],
        id_tokens: upland::oidc::IdTokenValidator::Fixed(Default::default()),
    }
}

fn test_secret() -> upland::auth::SigningSecret {
    upland::auth::SigningSecret::from_hex(
        "abababababababababababababababababababababababababababababababab",
    )
    .expect("test secret")
}

async fn test_server() -> TestServer {
    TestServer::new(upland::router(test_state().await))
}

async fn signup(server: &TestServer, username: &str, email: &str, password: &str) -> TestResponse {
    server
        .post("/api/v1/auth/signup")
        .json(&json!({ "username": username, "email": email, "password": password }))
        .await
}

#[tokio::test]
async fn api_consumer_receives_hello_at_the_versioned_prefix() {
    let server = test_server().await;
    let signup = signup(&server, "hello_friend", "hello@example.com", "password").await;
    let token = signup.json::<Value>()["access_token"]
        .as_str()
        .expect("token")
        .to_owned();
    let response = server
        .get("/api/v1/hello")
        .add_header("authorization", format!("Bearer {token}"))
        .await;

    response.assert_status_ok();
    assert_eq!(
        response.json::<Value>(),
        json!({ "message": "Hello from upland!" })
    );
}

#[tokio::test]
async fn the_unversioned_hello_path_is_gone_not_aliased() {
    // A clean break means the old path answers like any unknown API path;
    // an alias would let the version prefix quietly mean nothing.
    let server = test_server().await;
    let response = server.get("/api/hello").await;

    response.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(
        response.json::<Value>(),
        json!({ "error": { "code": "not_found" } })
    );
}

#[tokio::test]
async fn api_consumer_gets_json_not_found_for_unknown_api_path() {
    let server = test_server().await;
    let response = server.get("/api/nope").await;

    response.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(
        response.json::<Value>(),
        json!({ "error": { "code": "not_found" } })
    );
}

#[tokio::test]
async fn unknown_paths_under_the_version_prefix_answer_only_to_a_token() {
    // Spec #13: everything under /api/v1 requires a bearer token by
    // default — the exemption list is named routes, not "known routes",
    // so an unseen path behind the prefix fails like a guarded one.
    let server = test_server().await;
    server
        .get("/api/v1/nope")
        .await
        .assert_status_unauthorized();

    let response = signup(&server, "Fallback", "fallback@example.com", "password").await;
    let token = response.json::<Value>()["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let authenticated = server
        .get("/api/v1/nope")
        .add_header("authorization", format!("Bearer {token}"))
        .await;
    authenticated.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(
        authenticated.json::<Value>(),
        json!({ "error": { "code": "not_found" } })
    );
}

#[tokio::test]
async fn operator_can_check_health() {
    let server = test_server().await;
    let response = server.get("/healthz").await;

    response.assert_status_ok();
    assert_eq!(response.json::<Value>(), json!({ "status": "ok" }));
}

#[tokio::test]
async fn signup_creates_the_account_and_answers_an_access_token() {
    let server = test_server().await;
    let response = signup(&server, "Ada", "Ada@Example.COM", "hunter2").await;
    response.assert_status_ok();
    let token = response.json::<Value>()["access_token"]
        .as_str()
        .expect("access token")
        .to_owned();

    server
        .get("/api/v1/hello")
        .add_header("authorization", format!("Bearer {token}"))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn the_versioned_api_demands_a_bearer_token() {
    let server = test_server().await;

    let anonymous = server.get("/api/v1/hello").await;
    anonymous.assert_status_unauthorized();
    assert_eq!(
        anonymous.json::<Value>(),
        json!({ "error": { "code": "unauthorized" } })
    );

    let forged = server
        .get("/api/v1/hello")
        .add_header("authorization", "Bearer not.a.jwt")
        .await;
    forged.assert_status_unauthorized();

    let no_scheme = server
        .get("/api/v1/hello")
        .add_header("authorization", "Basic dXNlcjpwYXNz")
        .await;
    no_scheme.assert_status_unauthorized();
}

#[tokio::test]
async fn health_and_the_exempt_auth_routes_stay_open() {
    // The login screen must be able to ask who exists before anyone has a
    // token; the operator probe never needs one.
    let server = test_server().await;
    server.get("/healthz").await.assert_status_ok();
    signup(&server, "Probe", "probe@example.com", "password")
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn signup_collides_case_insensitively_on_both_handles() {
    let server = test_server().await;
    signup(&server, "Bob", "bob@example.com", "password")
        .await
        .assert_status_ok();

    let username = signup(&server, "bob", "other@example.com", "password").await;
    username.assert_status(StatusCode::CONFLICT);
    assert_eq!(
        username.json::<Value>(),
        json!({ "error": { "code": "username_taken", "field": "username" } })
    );

    let email = signup(&server, "carol", "BOB@example.com", "password").await;
    email.assert_status(StatusCode::CONFLICT);
    assert_eq!(
        email.json::<Value>(),
        json!({ "error": { "code": "email_taken", "field": "email" } })
    );
}

#[tokio::test]
async fn signup_validates_every_field_by_name() {
    let server = test_server().await;
    let cases: Vec<(String, String, String, &'static str)> = vec![
        (
            "B!".to_owned(),
            "x@e.co".to_owned(),
            "password".to_owned(),
            "username",
        ),
        (
            "ab".to_owned(),
            "x@e.co".to_owned(),
            "password".to_owned(),
            "username",
        ),
        (
            format!("u{}", "a".repeat(32)),
            "x@e.co".to_owned(),
            "password".to_owned(),
            "username",
        ),
        (
            "good_one".to_owned(),
            "nope".to_owned(),
            "password".to_owned(),
            "email",
        ),
        (
            "good_two".to_owned(),
            "@e.co".to_owned(),
            "password".to_owned(),
            "email",
        ),
        (
            "good_three".to_owned(),
            "a@".to_owned(),
            "password".to_owned(),
            "email",
        ),
        (
            "good_four".to_owned(),
            "x@e.co".to_owned(),
            String::new(),
            "password",
        ),
        (
            "good_five".to_owned(),
            "x@e.co".to_owned(),
            "p".repeat(73),
            "password",
        ),
    ];
    for (username, email, password, field) in cases {
        let response = signup(&server, &username, &email, &password).await;
        response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            response.json::<Value>(),
            json!({ "error": { "code": "invalid", "field": field } }),
            "{username:?}/{email:?}/{password:?}"
        );
    }
}

#[tokio::test]
async fn a_body_missing_its_fields_names_the_first_absent_one() {
    let server = test_server().await;
    let response = server
        .post("/api/v1/auth/signup")
        .json(&json!({ "username": "just_a_name" }))
        .await;
    response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response.json::<Value>(),
        json!({ "error": { "code": "invalid", "field": "email" } })
    );
}

#[tokio::test]
async fn an_expired_token_is_rejected() {
    let state = test_state().await;
    let server = TestServer::new(upland::router(state.clone()));
    signup(&server, "Expiry", "expiry@example.com", "password")
        .await
        .assert_status_ok();

    let past = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
        - 60;
    let expired = upland::auth::AccessTokenIssuer::new(test_secret(), 0)
        .mint_at("1", past)
        .expect("expired token");
    server
        .get("/api/v1/hello")
        .add_header("authorization", format!("Bearer {expired}"))
        .await
        .assert_status_unauthorized();
}

#[tokio::test]
async fn a_token_for_a_vanished_account_is_rejected() {
    let state = test_state().await;
    let server = TestServer::new(upland::router(state.clone()));
    let response = signup(&server, "GoneSoon", "gone@example.com", "password").await;
    response.assert_status_ok();
    let token = response.json::<Value>()["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    server
        .get("/api/v1/hello")
        .add_header("authorization", format!("Bearer {token}"))
        .await
        .assert_status_ok();

    sqlx::query!("DELETE FROM accounts WHERE id = 1")
        .execute(state.db.writer())
        .await
        .expect("delete");

    server
        .get("/api/v1/hello")
        .add_header("authorization", format!("Bearer {token}"))
        .await
        .assert_status_unauthorized();
}

#[tokio::test]
async fn signup_stores_only_the_hash_and_lowercase_handles() {
    let state = test_state().await;
    let server = TestServer::new(upland::router(state.clone()));
    signup(&server, "Mira", "Mira@Example.COM", "hunter2")
        .await
        .assert_status_ok();

    let row: (String, String, String) = sqlx::query_as(
        "SELECT a.username, a.email, c.password_hash
         FROM accounts a JOIN credentials c ON c.account_id = a.id",
    )
    .fetch_one(state.db.reader())
    .await
    .expect("account row");
    assert_eq!(row.0, "mira");
    assert_eq!(row.1, "mira@example.com");
    assert!(row.2.starts_with("$2b$12$"), "{}", row.2);
    assert!(!row.2.contains("hunter2"));
}

async fn login(server: &TestServer, username: &str, password: &str) -> TestResponse {
    server
        .post("/api/v1/auth/login")
        .json(&json!({ "username": username, "password": password }))
        .await
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

#[tokio::test]
async fn login_answers_a_working_access_token() {
    let server = test_server().await;
    signup(&server, "Reta", "reta@example.com", "hunter2").await;

    let response = login(&server, "reta", "hunter2").await;
    response.assert_status_ok();
    let token = response.json::<Value>()["access_token"]
        .as_str()
        .expect("token")
        .to_owned();
    server
        .get("/api/v1/hello")
        .add_header("authorization", bearer(&token))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn login_refuses_unknown_users_and_wrong_passwords_identically() {
    let server = test_server().await;
    signup(&server, "Known", "known@example.com", "hunter2").await;

    let unknown = login(&server, "nobody", "hunter2").await;
    let wrong = login(&server, "known", "wrong-one").await;
    for response in [unknown, wrong] {
        response.assert_status_unauthorized();
        assert_eq!(
            response.json::<Value>(),
            json!({ "error": { "code": "unauthorized" } })
        );
    }
}

#[tokio::test]
async fn an_account_without_a_password_door_still_401s_by_name() {
    // OIDC-created accounts have no credentials row (ADR-0004); a password
    // login must fail them exactly like every other failure.
    let state = test_state().await;
    let server = TestServer::new(upland::router(state.clone()));
    signup(&server, "Passwordless", "pless@example.com", "hunter2")
        .await
        .assert_status_ok();
    sqlx::query!("DELETE FROM credentials WHERE account_id = 1")
        .execute(state.db.writer())
        .await
        .expect("drop the credential");

    let response = login(&server, "passwordless", "hunter2").await;
    response.assert_status_unauthorized();
    assert_eq!(
        response.json::<Value>(),
        json!({ "error": { "code": "unauthorized" } })
    );
}

#[tokio::test]
async fn login_body_that_omits_a_field_names_it() {
    let server = test_server().await;
    let response = server
        .post("/api/v1/auth/login")
        .json(&json!({ "username": "someone" }))
        .await;
    response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response.json::<Value>(),
        json!({ "error": { "code": "invalid", "field": "password" } })
    );
}

#[tokio::test]
async fn me_answers_the_account_it_was_minted_for() {
    let server = test_server().await;
    let signup = signup(&server, "Tess", "Tess@Example.com", "hunter2").await;
    let token = signup.json::<Value>()["access_token"]
        .as_str()
        .unwrap()
        .to_owned();

    let anonymous = server.get("/api/v1/auth/me").await;
    anonymous.assert_status_unauthorized();

    let response = server
        .get("/api/v1/auth/me")
        .add_header("authorization", bearer(&token))
        .await;
    response.assert_status_ok();
    assert_eq!(
        response.json::<Value>(),
        json!({
            "id": 1,
            "username": "tess",
            "email": "tess@example.com",
            "identities": []
        })
    );
}

#[tokio::test]
async fn logout_requires_a_token_and_revokes_nothing_but_forgives_nothing() {
    let server = test_server().await;
    server
        .post("/api/v1/auth/logout")
        .await
        .assert_status_unauthorized();

    let signup = signup(&server, "Leaver", "leaver@example.com", "hunter2").await;
    let token = signup.json::<Value>()["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    server
        .post("/api/v1/auth/logout")
        .add_header("authorization", bearer(&token))
        .await
        .assert_status_ok();

    // The honest half of stateless: the token outlives the logout.
    server
        .get("/api/v1/hello")
        .add_header("authorization", bearer(&token))
        .await
        .assert_status_ok();
}

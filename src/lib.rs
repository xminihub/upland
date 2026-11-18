use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::http::Uri;
use axum::http::header::CONTENT_TYPE;
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::routing::post;
use rust_embed::RustEmbed;
use serde_json::{Value, json};

use crate::auth::AccessTokenIssuer;

pub mod auth;
pub mod db;
pub mod oidc;

use auth::SigningSecret;
use db::Db;

#[derive(RustEmbed)]
#[folder = "web/dist"]
pub struct Assets;

pub fn spa_enabled() -> bool {
    Assets::iter().next().is_some()
}

/// Everything a handler may reach without taking arguments. The **database**
/// lives here so the persistence shape (ADR-0003) is decided once, at
/// construction, and never inside a handler.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    /// The **access token** mint and its guard (ADR-0004): the **signing
    /// secret** and TTL arrive once from the **config file**, never from a
    /// handler.
    pub access_tokens: AccessTokenIssuer,
    /// The configured **provider**s (ADR-0005), names validated at boot.
    pub oidc_providers: Vec<OidcProvider>,
    /// The ID-token verification boundary (ADR-0004): live against the
    /// configured issuers in production, fixtures in tests.
    pub id_tokens: oidc::IdTokenValidator,
}

pub fn router(state: AppState) -> Router {
    let router = Router::<AppState>::new()
        .nest("/api", api(state.clone()))
        .route("/healthz", get(healthz))
        .with_state(state);
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

fn api(state: AppState) -> Router<AppState> {
    let public = Router::new()
        .route("/auth/signup", post(signup))
        .route("/auth/login", post(login))
        .route("/auth/oidc", post(oidc_exchange))
        .route("/auth/providers", get(providers));
    let guarded = Router::new()
        .route("/hello", get(hello))
        .route("/auth/me", get(me))
        .route("/auth/logout", post(logout))
        .route_layer(from_fn_with_state(state, require_bearer));
    Router::new()
        .nest("/v1", public.merge(guarded).fallback(api_not_found))
        .fallback(api_not_found)
}

/// The verified **access token**'s claims, published to handlers by the
/// bearer middleware through the request's extensions.
#[derive(Clone)]
pub struct Authenticated(pub auth::Claims);

impl axum::extract::FromRequestParts<AppState> for Authenticated {
    type Rejection = (StatusCode, Json<Value>);

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Authenticated>()
            .cloned()
            .ok_or_else(unauthorized_pair)
    }
}

async fn require_bearer(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Response {
    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    let claims = match state.access_tokens.verify(bearer) {
        Ok(claims) => claims,
        Err(_) => return unauthorized(),
    };
    let Ok(subject) = claims.sub.parse::<i64>() else {
        return unauthorized();
    };
    let alive = sqlx::query!("SELECT id FROM accounts WHERE id = ?", subject)
        .fetch_optional(state.db.reader())
        .await;
    match alive {
        Ok(Some(_)) => {
            request.extensions_mut().insert(Authenticated(claims));
            next.run(request).await
        }
        _ => unauthorized(),
    }
}

fn unauthorized_body() -> Value {
    json!({ "error": { "code": "unauthorized" } })
}

fn unauthorized_pair() -> (StatusCode, Json<Value>) {
    (StatusCode::UNAUTHORIZED, Json(unauthorized_body()))
}

fn unauthorized() -> Response {
    unauthorized_pair().into_response()
}

#[derive(serde::Deserialize)]
struct SignupBody {
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    password: Option<String>,
}

async fn signup(
    State(state): State<AppState>,
    payload: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> (StatusCode, Json<Value>) {
    let Ok(Json(body)) = payload else {
        return invalid(None);
    };
    let Ok(parsed) = serde_json::from_value::<SignupBody>(body) else {
        return invalid(None);
    };
    let Some(username) = parsed.username else {
        return invalid(Some("username"));
    };
    let Some(email) = parsed.email else {
        return invalid(Some("email"));
    };
    let Some(password) = parsed.password else {
        return invalid(Some("password"));
    };

    // Field rules (spec #13): ASCII minimal and explicit, mirrored by the
    // SPA forms; the table's NOCASE uniques are the last word.
    let username = username.trim().to_lowercase();
    let valid_username = (3..=32).contains(&username.len())
        && username
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-_".contains(&b));
    if !valid_username {
        return invalid(Some("username"));
    }

    let email = email.trim().to_lowercase();
    let valid_email = match email.split_once('@') {
        Some((local, domain)) => !local.is_empty() && !domain.is_empty() && !domain.contains('@'),
        None => false,
    };
    if !valid_email {
        return invalid(Some("email"));
    }

    // bcrypt silently truncates at 72 bytes; a password that long was
    // never the one that would be checked at login.
    let valid_password = !password.is_empty() && password.len() <= 72;
    if !valid_password {
        return invalid(Some("password"));
    }

    if let Some(taken) = taken_handle(&state, &username, &email).await {
        return conflict(taken);
    }

    let hashing = tokio::task::spawn_blocking(move || bcrypt::hash(&password, 12));
    let Ok(Ok(hash)) = hashing.await else {
        return internal();
    };

    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after 1970")
        .as_secs() as i64;
    let inserted = sqlx::query!(
        "INSERT INTO accounts (username, email, created_at) VALUES (?, ?, ?) RETURNING id",
        username,
        email,
        created_at
    )
    .fetch_one(state.db.writer())
    .await;
    let row = match inserted {
        Ok(row) => row,
        Err(error) => {
            // The race the pre-check cannot close: two boots claim one
            // handle, the database says one.
            if let Some(taken) = collision_of(&error) {
                return conflict(taken);
            }
            return internal();
        }
    };
    let id = row.id;
    if sqlx::query!(
        "INSERT INTO credentials (account_id, password_hash) VALUES (?, ?)",
        id,
        hash.to_string()
    )
    .execute(state.db.writer())
    .await
    .is_err()
    {
        // Roll the half-account back: an account nobody can enter is a
        // taken handle held hostage. The logs table this notice would
        // write to is still unbuilt (ADR-0003).
        let _ = sqlx::query!("DELETE FROM accounts WHERE id = ?", id)
            .execute(state.db.writer())
            .await;
        return internal();
    }

    match state.access_tokens.mint(&id.to_string()) {
        Ok(token) => (StatusCode::OK, Json(json!({ "access_token": token }))),
        Err(_) => internal(),
    }
}

async fn taken_handle(state: &AppState, username: &str, email: &str) -> Option<&'static str> {
    if sqlx::query!("SELECT id FROM accounts WHERE username = ?", username)
        .fetch_optional(state.db.reader())
        .await
        .ok()?
        .is_some()
    {
        return Some("username_taken");
    }
    if sqlx::query!("SELECT id FROM accounts WHERE email = ?", email)
        .fetch_optional(state.db.reader())
        .await
        .ok()?
        .is_some()
    {
        return Some("email_taken");
    }
    None
}

fn collision_of(error: &sqlx::Error) -> Option<&'static str> {
    let text = error.to_string();
    if text.contains("accounts.username") {
        Some("username_taken")
    } else if text.contains("accounts.email") {
        Some("email_taken")
    } else {
        None
    }
}

fn conflict(code: &str) -> (StatusCode, Json<Value>) {
    let field = if code == "username_taken" {
        "username"
    } else {
        "email"
    };
    (
        StatusCode::CONFLICT,
        Json(json!({ "error": { "code": code, "field": field } })),
    )
}

fn internal() -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": { "code": "internal" } })),
    )
}

fn invalid(field: Option<&str>) -> (StatusCode, Json<Value>) {
    let error = match field {
        Some(field) => json!({ "error": { "code": "invalid", "field": field } }),
        None => json!({ "error": { "code": "invalid" } }),
    };
    (StatusCode::UNPROCESSABLE_ENTITY, Json(error))
}

#[derive(serde::Deserialize)]
struct LoginBody {
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    password: Option<String>,
}

/// Passwords fail for one reason only: the pair does not verify. Whether
/// the username exists, whether it has a credential at all (ADR-0004
/// lets OIDC-only accounts exist), and whether the password was merely
/// wrong all answer the same 401 — the door does not explain the lock.
async fn login(
    State(state): State<AppState>,
    payload: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> (StatusCode, Json<Value>) {
    let Ok(Json(body)) = payload else {
        return invalid(None);
    };
    let Ok(parsed) = serde_json::from_value::<LoginBody>(body) else {
        return invalid(None);
    };
    let Some(username) = parsed.username else {
        return invalid(Some("username"));
    };
    let Some(password) = parsed.password else {
        return invalid(Some("password"));
    };

    let username = username.trim().to_lowercase();
    let account = sqlx::query!("SELECT id FROM accounts WHERE username = ?", username)
        .fetch_optional(state.db.reader())
        .await
        .ok()
        .flatten();
    let Some(account) = account else {
        return unauthorized_pair();
    };
    let credential = sqlx::query!(
        "SELECT password_hash FROM credentials WHERE account_id = ?",
        account.id
    )
    .fetch_optional(state.db.reader())
    .await
    .ok()
    .flatten();
    let Some(credential) = credential else {
        return unauthorized_pair();
    };

    let checking =
        tokio::task::spawn_blocking(move || bcrypt::verify(&password, &credential.password_hash));
    let verified = matches!(checking.await, Ok(Ok(true)));
    if !verified {
        return unauthorized_pair();
    }

    match state.access_tokens.mint(&account.id.to_string()) {
        Ok(token) => (StatusCode::OK, Json(json!({ "access_token": token }))),
        Err(_) => internal(),
    }
}

async fn me(
    State(state): State<AppState>,
    Authenticated(claims): Authenticated,
) -> (StatusCode, Json<Value>) {
    let Ok(account_id) = claims.sub.parse::<i64>() else {
        return unauthorized_pair();
    };
    let Ok(Some(account)) = sqlx::query!(
        "SELECT id, username, email FROM accounts WHERE id = ?",
        account_id
    )
    .fetch_optional(state.db.reader())
    .await
    else {
        return unauthorized_pair();
    };
    let identities = sqlx::query!(
        "SELECT provider_name FROM identities WHERE account_id = ? ORDER BY provider_name",
        account_id
    )
    .fetch_all(state.db.reader())
    .await
    .unwrap_or_default();

    (
        StatusCode::OK,
        Json(json!({
            "id": account.id,
            "username": account.username,
            "email": account.email,
            "identities": identities
                .iter()
                .map(|row| row.provider_name.clone())
                .collect::<Vec<String>>(),
        })),
    )
}

/// The client's forget ritual (ADR-0004): stateless logout revokes
/// nothing, and the API says so rather than pretending.
async fn logout(Authenticated(_claims): Authenticated) -> (StatusCode, Json<Value>) {
    (StatusCode::OK, Json(json!({ "ok": true })))
}

/// The exempt read the login screen needs before anyone has credentials
/// (ADR-0004): which doors exist, and what the browser needs to knock on
/// them directly — issuer and client id, never a secret.
async fn providers(State(state): State<AppState>) -> Json<Value> {
    let listing = state
        .oidc_providers
        .iter()
        .map(|provider| {
            json!({
                "name": provider.name,
                "issuer": provider.issuer,
                "client_id": provider.client_id,
            })
        })
        .collect::<Vec<Value>>();
    Json(json!(listing))
}

#[derive(serde::Deserialize)]
struct OidcBody {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
}

/// The second door (ADR-0004): the SPA's PKCE result is verified against
/// the configured issuer, and the account it names — matched by email,
/// created if novel — receives an upland token like any other login.
async fn oidc_exchange(
    State(state): State<AppState>,
    payload: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> (StatusCode, Json<Value>) {
    let Ok(Json(body)) = payload else {
        return invalid(None);
    };
    let Ok(parsed) = serde_json::from_value::<OidcBody>(body) else {
        return invalid(None);
    };
    let Some(provider_name) = parsed.provider else {
        return invalid(Some("provider"));
    };
    let Some(id_token) = parsed.id_token else {
        return invalid(Some("id_token"));
    };
    let Some(provider) = state
        .oidc_providers
        .iter()
        .find(|candidate| candidate.name == provider_name)
        .cloned()
    else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": { "code": "provider_unknown", "field": "provider" } })),
        );
    };

    let Ok(identity) = state
        .id_tokens
        .validate(&provider, &id_token, parsed.nonce.as_deref())
        .await
    else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": { "code": "oidc_rejected" } })),
        );
    };

    // The identity has signed in before: its account, directly.
    if let Ok(Some(row)) = sqlx::query!(
        "SELECT account_id FROM identities WHERE issuer = ? AND subject = ?",
        provider.issuer,
        identity.subject
    )
    .fetch_optional(state.db.reader())
    .await
    {
        return token_for(&state, row.account_id).await;
    }

    // Email is the linking key, verified claim or not (ADR-0004); with no
    // email asserted there is nothing to link by, and the provider simply
    // does not get an account.
    let Some(email) = identity
        .email
        .as_deref()
        .map(|email| email.trim().to_lowercase())
    else {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": { "code": "provider_no_email" } })),
        );
    };

    if let Ok(Some(row)) = sqlx::query!("SELECT id FROM accounts WHERE email = ?", email)
        .fetch_optional(state.db.reader())
        .await
    {
        if sqlx::query!(
            "INSERT INTO identities (provider_name, issuer, subject, account_id) VALUES (?, ?, ?, ?)",
            provider.name,
            provider.issuer,
            identity.subject,
            row.id
        )
        .execute(state.db.writer())
        .await
        .is_err()
        {
            return internal();
        }
        return token_for(&state, row.id).await;
    }

    let base = derived_username(&email);
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after 1970")
        .as_secs() as i64;
    for attempt in 0..100u32 {
        let username = candidate_username(&base, attempt);
        let inserted = sqlx::query!(
            "INSERT INTO accounts (username, email, created_at) VALUES (?, ?, ?) RETURNING id",
            username,
            email,
            created_at
        )
        .fetch_one(state.db.writer())
        .await;
        match inserted {
            Ok(row) => {
                if sqlx::query!(
                    "INSERT INTO identities (provider_name, issuer, subject, account_id) VALUES (?, ?, ?, ?)",
                    provider.name,
                    provider.issuer,
                    identity.subject,
                    row.id
                )
                .execute(state.db.writer())
                .await
                .is_err()
                {
                    let _ = sqlx::query!("DELETE FROM accounts WHERE id = ?", row.id)
                        .execute(state.db.writer())
                        .await;
                    return internal();
                }
                return token_for(&state, row.id).await;
            }
            Err(error) => {
                let taken_by_username = error.to_string().contains("accounts.username");
                let taken_by_email = error.to_string().contains("accounts.email");
                if taken_by_username {
                    continue;
                }
                if taken_by_email {
                    // Raced with a password signup of the same email:
                    // link to that account instead of failing.
                    if let Ok(Some(row)) =
                        sqlx::query!("SELECT id FROM accounts WHERE email = ?", email)
                            .fetch_optional(state.db.reader())
                            .await
                    {
                        let _ = sqlx::query!(
                            "INSERT INTO identities (provider_name, issuer, subject, account_id) VALUES (?, ?, ?, ?)",
                            provider.name,
                            provider.issuer,
                            identity.subject,
                            row.id
                        )
                        .execute(state.db.writer())
                        .await;
                        return token_for(&state, row.id).await;
                    }
                }
                return internal();
            }
        }
    }
    internal()
}

async fn token_for(state: &AppState, account_id: i64) -> (StatusCode, Json<Value>) {
    match state.access_tokens.mint(&account_id.to_string()) {
        Ok(token) => (StatusCode::OK, Json(json!({ "access_token": token }))),
        Err(_) => internal(),
    }
}

/// The handle an OIDC account is born with (ADR-0004): the email's local
/// part, sanitized to the username rules. The person never chose it; a
/// future rename feature will let them.
fn derived_username(email: &str) -> String {
    let local = email.split('@').next().unwrap_or(email);
    let sanitized: String = local
        .chars()
        .filter(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '_'
        })
        .collect();
    let trimmed = sanitized.trim_matches(|c| c == '.' || c == '-' || c == '_');
    if trimmed.is_empty() {
        "oidc".to_owned()
    } else {
        trimmed.chars().take(28).collect()
    }
}

fn candidate_username(base: &str, attempt: u32) -> String {
    if attempt == 0 {
        base.to_owned()
    } else {
        // Suffixes count from -2: "grace" taken yields "grace-2", never
        // "grace-1" beside a name that never had a suffix to begin with.
        let suffix = format!("-{}", attempt + 1);
        let keep = 32usize.saturating_sub(suffix.len());
        format!("{}{}", base.chars().take(keep).collect::<String>(), suffix)
    }
}

async fn api_not_found() -> (StatusCode, Json<Value>) {
    json_not_found()
}

fn json_not_found() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": { "code": "not_found" } })),
    )
}

async fn hello() -> Json<Value> {
    Json(json!({ "message": "Hello from upland!" }))
}

async fn healthz() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 8080;

#[derive(Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    /// The **database path** (CONTEXT.md): one SQLite file, defaulting to a
    /// per-user data directory so `brew services` and a bare `upland` run
    /// start in the same place (ADR-0003).
    pub db_path: PathBuf,
    /// The **config path** (CONTEXT.md, ADR-0005): where the **config file**
    /// was read from, defaulting beside the **database path**.
    pub config_path: PathBuf,
    /// The **signing secret** (ADR-0004): mints and verifies every
    /// **access token**. Generated on first boot, operator-owned after.
    pub signing_secret: SigningSecret,
    /// The **access token** lifetime in seconds (ADR-0004), 86400 unless
    /// the **config file** says otherwise.
    pub ttl_seconds: u64,
    /// The configured **provider**s (ADR-0005): parsed strictly,
    /// validated and served by the OIDC feature.
    pub oidc_providers: Vec<OidcProvider>,
}

pub const DEFAULT_TTL_SECONDS: u64 = 86400;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OidcProvider {
    pub name: String,
    pub issuer: String,
    pub client_id: String,
}

/// The per-user data directory a platform expects a server's state in. No
/// `directories` dependency: three known paths, each resolved from
/// environment a launcher already sets.
fn data_directory(env: &impl Fn(&str) -> Option<String>) -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    let dir = env("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(target_os = "windows")]
    let dir = env("APPDATA").map(PathBuf::from);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let dir = env("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|home| PathBuf::from(home).join(".local/share")));

    let dir = dir.filter(|dir| !dir.as_os_str().is_empty());
    dir.ok_or_else(|| {
        "cannot resolve a default data directory: no data directory in the environment; set --db-path or UPLAND_DB_PATH"
            .to_owned()
    })
}

fn default_db_path(env: &impl Fn(&str) -> Option<String>) -> Result<PathBuf, String> {
    Ok(data_directory(env)?.join("upland").join("upland.db"))
}

fn default_config_path(env: &impl Fn(&str) -> Option<String>) -> Result<PathBuf, String> {
    Ok(data_directory(env)?.join("upland").join("config.json"))
}

/// The **config file** (ADR-0005): strict JSON — an unknown key names
/// itself and fails the boot, because a silently ignored `oidc_provideers`
/// means an issuer nobody configured.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    #[serde(default)]
    server: FileServer,
    #[serde(default)]
    auth: FileAuth,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FileServer {
    host: Option<String>,
    port: Option<u64>,
    db_path: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FileAuth {
    jwt_secret: Option<String>,
    ttl_seconds: Option<u64>,
    #[serde(default)]
    oidc_providers: Vec<OidcProvider>,
}

/// The document a first boot leaves behind (ADR-0005): every default
/// spelled out, the **signing secret** generated, so the file an operator
/// edits is the file the server read.
fn default_config_document(
    env: &impl Fn(&str) -> Option<String>,
    secret: &str,
) -> Result<String, String> {
    let document = serde_json::json!({
        "server": {
            "host": DEFAULT_HOST,
            "port": DEFAULT_PORT,
            "db_path": default_db_path(env)?.display().to_string(),
        },
        "auth": {
            "jwt_secret": secret,
            "ttl_seconds": DEFAULT_TTL_SECONDS,
            "oidc_providers": [],
        },
    });
    serde_json::to_string_pretty(&document)
        .map(|text| format!("{text}\n"))
        .map_err(|error| error.to_string())
}

fn read_config_file(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "cannot read config file {}: {error}",
            path.display()
        )),
    }
}

fn create_config_file(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        // A racing boot won and wrote its own secret; the re-read below
        // resolves both servers onto the same file.
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
        Err(error) => {
            return Err(format!(
                "cannot create config file {}: {error}",
                path.display()
            ));
        }
    };
    use std::io::Write;
    file.write_all(contents.as_bytes())
        .map_err(|error| format!("cannot write config file {}: {error}", path.display()))
}

/// The storage seam (ADR-0005): `read_or_create(path, None)` reads;
/// `read_or_create(path, Some(contents))` creates exclusively when
/// absent and answers whatever the file now holds — the generated text
/// or a racing boot's.
fn read_or_create_config_file(path: &Path, write: Option<&str>) -> Result<Option<String>, String> {
    if let Some(contents) = write {
        create_config_file(path, contents)?;
    }
    read_config_file(path)
}

fn parse_file_config(path: &Path, contents: &str) -> Result<FileConfig, String> {
    serde_json::from_str(contents)
        .map_err(|error| format!("invalid config file {}: {error}", path.display()))
}

pub enum Invocation {
    Serve(Config),
    Version,
    Help,
}

pub fn version_line() -> String {
    format!("upland {}", env!("CARGO_PKG_VERSION"))
}

pub fn usage() -> String {
    format!(
        "Usage: upland [OPTIONS]

Options:
  -h, --help           Print this help
  -V, --version        Print the version
      --host <host>    Bind address (default {DEFAULT_HOST})
      --port <port>    Listen port (default {DEFAULT_PORT})
      --db-path <path> Database file (default: a per-user data directory)
      --config <path>  Config file (default: upland/config.json beside it)

Environment:
  UPLAND_HOST, UPLAND_PORT, UPLAND_DB_PATH
      Defaults for --host, --port and --db-path; the flags win.
  UPLAND_CONFIG
      Default for --config; the flag wins. Precedence overall is
      flag, then environment, then config file, then built-in default.
"
    )
}

fn parse_port(value: &str) -> Result<u16, String> {
    let port = value
        .parse::<u16>()
        .map_err(|_| format!("not a port: {value}"))?;
    if port == 0 {
        return Err(format!("not a port: {value}"));
    }
    Ok(port)
}

fn flag_value<'a>(args: &'a [String], index: usize, name: &str) -> Result<&'a String, String> {
    args.get(index + 1)
        .filter(|value| !value.is_empty() && !value.starts_with('-'))
        .ok_or_else(|| format!("{name} requires a value"))
}

fn bind_and_storage<I, S, E, R>(args: I, env: &E, storage: &R) -> Result<Config, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
    E: Fn(&str) -> Option<String>,
    R: Fn(&Path, Option<&str>) -> Result<Option<String>, String>,
{
    let args: Vec<String> = args.into_iter().map(Into::into).collect();
    let mut host: Option<String> = None;
    let mut port: Option<u16> = None;
    let mut db_path: Option<String> = None;
    let mut config_path: Option<String> = None;

    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        match flag {
            "--host" => {
                host = Some(flag_value(&args, index, flag)?.clone());
                index += 2;
            }
            "--port" => {
                let value = flag_value(&args, index, flag)?;
                port = Some(parse_port(value)?);
                index += 2;
            }
            "--db-path" => {
                db_path = Some(flag_value(&args, index, flag)?.clone());
                index += 2;
            }
            "--config" => {
                config_path = Some(flag_value(&args, index, flag)?.clone());
                index += 2;
            }
            other => return Err(format!("unknown option: {other}")),
        }
    }

    // The config path itself is resolved flag > env > default; an explicit
    // value that is empty is a failure, and an explicit file that is absent
    // is a failure — a named path is a promise, not a hint.
    let explicit_config = config_path.clone().or_else(|| env("UPLAND_CONFIG"));
    if let Some(value) = explicit_config.as_ref()
        && value.trim().is_empty()
    {
        return Err("not a config path: the value must not be empty".to_owned());
    }
    let explicit = explicit_config.is_some();
    let config_path = match explicit_config {
        Some(value) => PathBuf::from(value),
        None => default_config_path(env)?,
    };
    let contents = match storage(&config_path, None)? {
        Some(contents) => contents,
        None => {
            if explicit {
                return Err(format!("config file not found: {}", config_path.display()));
            }
            // ADR-0005: a default location that has never held a file gets
            // one, secret included. A racing boot's file wins the exclusive
            // create; the re-read resolves both servers onto the same one.
            let generated = default_config_document(env, &SigningSecret::random().hex())?;
            storage(&config_path, Some(&generated))?
                .ok_or_else(|| format!("cannot create config file {}", config_path.display()))?
        }
    };
    let file = parse_file_config(&config_path, &contents)?;
    let server = file.server;

    // Precedence (ADR-0005): flag > environment > file > built-in default,
    // resolved per key.
    let host = host
        .or_else(|| env("UPLAND_HOST"))
        .or(server.host)
        .unwrap_or_else(|| DEFAULT_HOST.to_owned());
    if host.trim().is_empty() {
        return Err("not a host: the bind address must not be empty".to_owned());
    }
    // A value that cannot be a port is a failure rather than a nudge back to
    // the default, whether it arrived as a flag or in the environment: an
    // operator reading their own launchd plist would otherwise see the port
    // they wrote and believe it was the one in use.
    let port = match port {
        Some(port) => port,
        None => match env("UPLAND_PORT") {
            Some(value) => parse_port(&value)?,
            None => match server.port {
                Some(port) => parse_port(&port.to_string())
                    .map_err(|error| format!("config file server.port: {error}"))?,
                None => DEFAULT_PORT,
            },
        },
    };
    // An empty UPLAND_DB_PATH is a failure rather than a nudge back to the
    // default: a unit file that lost its path
    // mid-edit would otherwise open a database nobody pointed it at, in a
    // directory nobody chose.
    let db_path = match db_path.or_else(|| env("UPLAND_DB_PATH")).or(server.db_path) {
        Some(value) if value.trim().is_empty() => {
            return Err("not a database path: the value must not be empty".to_owned());
        }
        Some(value) => PathBuf::from(value),
        None => default_db_path(env)?,
    };

    // ADR-0004: the signing secret is generated on first boot and
    // operator-owned afterwards. A file that exists without one is a
    // half-written promise, and half a door is worse than none.
    let signing_secret = match file.auth.jwt_secret.as_deref() {
        None => Err(
            "config file auth.jwt_secret is missing: the signing secret every access token is minted with".to_owned()
        ),
        Some(value) if value.trim().is_empty() => Err(
            "config file auth.jwt_secret is empty: the signing secret every access token is minted with".to_owned()
        ),
        Some(value) => {
            SigningSecret::from_hex(value).map_err(|error| {
                format!("config file auth.jwt_secret: {error}")
            })
        }
    }?;
    let ttl_seconds = match file.auth.ttl_seconds {
        Some(0) => {
            return Err("config file auth.ttl_seconds must not be zero".to_owned());
        }
        Some(ttl_seconds) => ttl_seconds,
        None => DEFAULT_TTL_SECONDS,
    };
    // Provider names are quoted in API payloads and bookmarks forever
    // (ADR-0005): one charset, no duplicates, decided at the boot that
    // reads them rather than at the request that first trips over them.
    let mut seen_names = std::collections::HashSet::new();
    for provider in &file.auth.oidc_providers {
        let valid = !provider.name.is_empty()
            && provider.name.len() <= 32
            && provider
                .name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !valid {
            return Err(format!(
                "config file auth.oidc_providers: invalid provider name {:?}",
                provider.name
            ));
        }
        if !seen_names.insert(provider.name.clone()) {
            return Err(format!(
                "config file auth.oidc_providers: duplicate provider name {:?}",
                provider.name
            ));
        }
    }

    Ok(Config {
        host,
        port,
        db_path,
        config_path,
        signing_secret,
        ttl_seconds,
        oidc_providers: file.auth.oidc_providers,
    })
}

impl Config {
    /// The **listen address** (CONTEXT.md) as one string: the one place
    /// `host:port` is composed. An IPv6 host is bracketed, because `::1:8080`
    /// is a valid IPv6 literal that means something other than port 8080.
    pub fn listen_address(&self) -> String {
        let host = if self.host.contains(':') && !self.host.starts_with('[') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        format!("{host}:{}", self.port)
    }

    pub fn from_args<I, S, E>(args: I, env: E) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
        E: Fn(&str) -> Option<String>,
    {
        Self::from_args_with(args, env, read_or_create_config_file)
    }

    /// The same resolution with file access injected — reads and the
    /// first-boot create — so the whole boot story of the **config file**
    /// is testable without touching a real disk.
    pub fn from_args_with<I, S, E, R>(args: I, env: E, storage: R) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
        E: Fn(&str) -> Option<String>,
        R: Fn(&Path, Option<&str>) -> Result<Option<String>, String>,
    {
        bind_and_storage(args, &env, &storage)
    }
}

pub fn parse_args<I, S, E>(args: I, env: E) -> Result<Invocation, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
    E: Fn(&str) -> Option<String>,
{
    parse_args_with(args, env, read_or_create_config_file)
}

/// The same decision with file access injected, so the boot story is
/// testable without a real disk.
pub fn parse_args_with<I, S, E, R>(args: I, env: E, storage: R) -> Result<Invocation, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
    E: Fn(&str) -> Option<String>,
    R: Fn(&Path, Option<&str>) -> Result<Option<String>, String>,
{
    let args: Vec<String> = args.into_iter().map(Into::into).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        return Ok(Invocation::Version);
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(Invocation::Help);
    }
    Config::from_args_with(args, env, storage).map(Invocation::Serve)
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
        let server = TestServer::new(router(test_state().await));
        server
            .get("/nope")
            .await
            .assert_status(StatusCode::NOT_FOUND);
    }

    #[test]
    fn no_arguments_bind_loopback_on_the_default_port() {
        // A brew service and a laptop both start with nothing on the command
        // line: what they must not get is a port open on the network.
        let config = crate::Config::from_args_with(Vec::<String>::new(), env(&[]), fs(&[]))
            .expect("defaults");
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 8080);
        // The default database path is a per-user location, not the working
        // directory: every launcher would otherwise own the answer to "which
        // database did I just open" differently.
        assert_eq!(config.db_path.file_name().unwrap(), "upland.db");
    }

    #[test]
    fn the_default_database_path_follows_the_platform_data_directory() {
        #[cfg(target_os = "macos")]
        let expected =
            std::path::Path::new("/home/tester/Library/Application Support/upland/upland.db");
        #[cfg(target_os = "windows")]
        let expected =
            std::path::Path::new("C:\\Users\\tester\\AppData\\Roaming\\upland\\upland.db");
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let expected = std::path::Path::new("/home/tester/.local/share/upland/upland.db");

        let config = crate::Config::from_args_with(Vec::<String>::new(), env(&[]), fs(&[]))
            .expect("defaults");
        assert_eq!(config.db_path, expected);
    }

    #[test]
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn xdg_data_home_wins_over_the_local_share_fallback() {
        let config = crate::Config::from_args_with(
            Vec::<String>::new(),
            env(&[("XDG_DATA_HOME", "/srv/upland-data")]),
            fs(&[]),
        )
        .expect("xdg");
        assert_eq!(
            config.db_path,
            std::path::Path::new("/srv/upland-data/upland/upland.db")
        );
    }

    #[test]
    fn the_database_path_flag_and_environment_are_honoured() {
        let from_flag = crate::Config::from_args_with(
            vec!["--db-path".to_string(), "/tmp/flag.db".to_string()],
            env(&[("UPLAND_DB_PATH", "/tmp/env.db")]),
            fs(&[]),
        )
        .expect("flag");
        assert_eq!(from_flag.db_path, std::path::Path::new("/tmp/flag.db"));

        let from_env = crate::Config::from_args_with(
            Vec::<String>::new(),
            env(&[("UPLAND_DB_PATH", "/tmp/env.db")]),
            fs(&[]),
        )
        .expect("env");
        assert_eq!(from_env.db_path, std::path::Path::new("/tmp/env.db"));

        // An empty UPLAND_DB_PATH is a failure, not a quiet default: the
        // operator wrote a path here and losing it silently is how the wrong
        // database gets opened.
        let error = crate::Config::from_args_with(
            Vec::<String>::new(),
            env(&[("UPLAND_DB_PATH", "")]),
            fs(&[]),
        )
        .expect_err("must fail");
        assert!(error.contains("database path"), "{error}");
    }

    #[test]
    fn host_and_port_flags_are_honoured() {
        let config = crate::Config::from_args_with(
            vec![
                "--host".to_string(),
                "0.0.0.0".to_string(),
                "--port".to_string(),
                "9000".to_string(),
            ],
            env(&[]),
            fs(&[]),
        )
        .expect("flags");
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 9000);
    }

    #[test]
    fn flags_come_before_the_order_they_are_written_in() {
        let config = crate::Config::from_args_with(
            vec![
                "--port".to_string(),
                "9001".to_string(),
                "--host".to_string(),
                "::1".to_string(),
            ],
            env(&[]),
            fs(&[]),
        )
        .expect("flags");
        assert_eq!(config.host, "::1");
        assert_eq!(config.port, 9001);
    }

    #[test]
    fn environment_variables_apply_when_the_flags_are_absent() {
        let config = crate::Config::from_args_with(
            Vec::<String>::new(),
            env(&[("UPLAND_HOST", "0.0.0.0"), ("UPLAND_PORT", "9100")]),
            fs(&[]),
        )
        .expect("env");
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 9100);
    }

    #[test]
    fn a_flag_wins_over_the_environment_variable_it_disagrees_with() {
        let config = crate::Config::from_args_with(
            vec!["--port".to_string(), "7000".to_string()],
            env(&[("UPLAND_HOST", "0.0.0.0"), ("UPLAND_PORT", "9100")]),
            fs(&[]),
        )
        .expect("flag beats env");
        assert_eq!(config.port, 7000);
        assert_eq!(config.host, "0.0.0.0");
    }

    #[test]
    fn an_unusable_port_is_a_failure_and_not_a_default() {
        for port in ["not-a-number", "0", "65536"] {
            let error = crate::Config::from_args_with(
                vec!["--port".to_string(), port.to_string()],
                env(&[]),
                fs(&[]),
            )
            .expect_err("must fail");
            assert!(error.contains("port"), "{port} produced {error}");
        }
    }

    #[test]
    fn an_environment_port_that_is_not_a_number_is_a_failure_too() {
        // UPLAND_PORT is set by whoever launched us — a shell export, a
        // launchd plist. A value that cannot be a port must not quietly become
        // a server on the default port: the operator would read their own
        // plist, see the value they wrote, and believe it was in use.
        for value in ["http", "0", "65536", ""] {
            let error = crate::Config::from_args_with(
                Vec::<String>::new(),
                env(&[("UPLAND_PORT", value)]),
                fs(&[]),
            )
            .expect_err("must fail");
            assert!(error.contains("port"), "{value} produced {error}");
        }
    }

    #[test]
    fn an_ipv6_listen_address_is_bracketed() {
        // "::1:8080" is itself a legal IPv6 literal meaning something else, so
        // an unbracketed IPv6 host would bind an address nobody wrote down.
        let config = crate::Config::from_args_with(
            vec![
                "--host".to_string(),
                "::1".to_string(),
                "--port".to_string(),
                "9100".to_string(),
            ],
            env(&[]),
            fs(&[]),
        )
        .expect("ipv6 loopback");
        assert_eq!(config.listen_address(), "[::1]:9100");

        let ipv4 = crate::Config::from_args_with(
            Vec::<String>::new(),
            env(&[("UPLAND_HOST", "0.0.0.0")]),
            fs(&[]),
        )
        .expect("ipv4");
        assert_eq!(ipv4.listen_address(), "0.0.0.0:8080");
    }

    #[test]
    fn a_flag_without_its_value_is_a_failure() {
        for args in [
            vec!["--port".to_string()],
            vec!["--host".to_string()],
            vec![
                "--port".to_string(),
                "--host".to_string(),
                "0.0.0.0".to_string(),
            ],
            vec!["--port".to_string(), "".to_string()],
        ] {
            let error = crate::Config::from_args_with(args.clone(), env(&[]), fs(&[]))
                .expect_err("must fail");
            assert!(!error.is_empty(), "{args:?} produced no message");
        }
    }

    #[test]
    fn an_unknown_option_is_a_failure_naming_it() {
        let error = crate::Config::from_args_with(
            vec!["--addr".to_string(), "0.0.0.0".to_string()],
            env(&[]),
            fs(&[]),
        )
        .expect_err("must fail");
        assert_eq!(error, "unknown option: --addr");
    }

    #[test]
    fn an_empty_host_never_reaches_the_listener() {
        let from_flag = crate::Config::from_args_with(
            vec!["--host".to_string(), " ".to_string()],
            env(&[]),
            fs(&[]),
        )
        .expect_err("a blank host is not an address");
        assert!(from_flag.contains("host"), "{from_flag}");

        let from_env = crate::Config::from_args_with(
            Vec::<String>::new(),
            env(&[("UPLAND_HOST", "")]),
            fs(&[]),
        )
        .expect_err("an empty host is not an address");
        assert!(from_env.contains("host"), "{from_env}");
    }

    #[test]
    fn version_and_help_never_start_a_server() {
        // Formula/upland.rb's `test do` runs `upland --version` and matches its
        // own pinned version against the output, so --version must be a print,
        // and it must be reached even when the rest of the command line is bad.
        let version = crate::parse_args_with(vec!["--version".to_string()], env(&[]), fs(&[]))
            .expect("version");
        assert!(matches!(version, crate::Invocation::Version));
        let short = crate::parse_args_with(vec!["-V".to_string()], env(&[]), fs(&[]))
            .expect("short version");
        assert!(matches!(short, crate::Invocation::Version));

        let help =
            crate::parse_args_with(vec!["--help".to_string()], env(&[]), fs(&[])).expect("help");
        assert!(matches!(help, crate::Invocation::Help));
        let short_help =
            crate::parse_args_with(vec!["-h".to_string()], env(&[]), fs(&[])).expect("short help");
        assert!(matches!(short_help, crate::Invocation::Help));

        let bad = crate::parse_args_with(
            vec![
                "--version".to_string(),
                "--port".to_string(),
                "nope".to_string(),
            ],
            env(&[]),
            fs(&[]),
        )
        .expect("version beats a broken port");
        assert!(matches!(bad, crate::Invocation::Version));
    }

    #[test]
    fn serving_is_the_default_invocation() {
        let serve = crate::parse_args_with(
            vec!["--port".to_string(), "9100".to_string()],
            env(&[]),
            fs(&[]),
        )
        .expect("serve");
        match serve {
            crate::Invocation::Serve(config) => assert_eq!(config.port, 9100),
            _ => panic!("expected a server"),
        }
    }

    #[test]
    fn the_version_line_is_what_the_formula_matches() {
        assert_eq!(
            crate::version_line(),
            format!("upland {}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn usage_documents_the_flags_and_the_defaults() {
        let usage = crate::usage();
        for expected in [
            "--host",
            "--port",
            "--db-path",
            "--config",
            "--version",
            "--help",
            "UPLAND_PORT",
            "UPLAND_DB_PATH",
            "UPLAND_CONFIG",
            "127.0.0.1",
        ] {
            assert!(usage.contains(expected), "usage omits {expected}");
        }
    }

    #[test]
    fn the_config_file_applies_when_nothing_higher_speaks() {
        let config = crate::Config::from_args_with(
            vec!["--config".to_string(), "/tmp/upspeak.json".to_string()],
            env(&[]),
            fs(&[(
                "/tmp/upspeak.json",
                r#"{ "server": { "host": "0.0.0.0", "port": 9300, "db_path": "/tmp/from-file.db" }, "auth": { "jwt_secret": "abababababababababababababababababababababababababababababababab" } }"#,
            )]),
        )
        .expect("file");
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 9300);
        assert_eq!(config.db_path, std::path::Path::new("/tmp/from-file.db"));
    }

    #[test]
    fn the_environment_beats_the_config_file() {
        let config = crate::Config::from_args_with(
            vec!["--config".to_string(), "/tmp/upenv.json".to_string()],
            env(&[("UPLAND_PORT", "9400")]),
            fs(&[(
                "/tmp/upenv.json",
                r#"{ "server": { "port": 9301 }, "auth": { "jwt_secret": "abababababababababababababababababababababababababababababababab" } }"#,
            )]),
        )
        .expect("env beats file");
        assert_eq!(config.port, 9400);
    }

    #[test]
    fn flags_env_and_file_are_resolved_per_key() {
        // Each key takes the highest voice that speaks it; the file's
        // db_path survives underneath a flag port and an env host.
        let config = crate::Config::from_args_with(
            vec![
                "--port".to_string(),
                "7000".to_string(),
                "--config".to_string(),
                "/tmp/upmix.json".to_string(),
            ],
            env(&[("UPLAND_HOST", "10.0.0.1")]),
            fs(&[(
                "/tmp/upmix.json",
                r#"{ "server": { "host": "0.0.0.0", "port": 9302, "db_path": "/tmp/mixed.db" }, "auth": { "jwt_secret": "abababababababababababababababababababababababababababababababab" } }"#,
            )]),
        )
        .expect("mixed");
        assert_eq!(config.port, 7000);
        assert_eq!(config.host, "10.0.0.1");
        assert_eq!(config.db_path, std::path::Path::new("/tmp/mixed.db"));
    }

    #[test]
    fn an_unknown_config_key_fails_naming_the_key() {
        let error = crate::Config::from_args_with(
            vec!["--config".to_string(), "/tmp/upbad.json".to_string()],
            env(&[]),
            fs(&[("/tmp/upbad.json", r#"{ "server": { "hosts": "0.0.0.0" } }"#)]),
        )
        .expect_err("strict");
        assert!(error.contains("hosts"), "{error}");
        assert!(error.contains("config"), "{error}");
    }

    #[test]
    fn a_malformed_config_file_is_a_failure() {
        let error = crate::Config::from_args_with(
            vec!["--config".to_string(), "/tmp/upbroken.json".to_string()],
            env(&[]),
            fs(&[("/tmp/upbroken.json", "{ not json")]),
        )
        .expect_err("must fail");
        assert!(error.contains("config"), "{error}");
    }

    #[test]
    fn an_explicit_config_path_that_does_not_exist_fails_by_name() {
        // A default location may be missing; a path the operator named is
        // a promise, and a broken promise is a failure, not a fallback.
        let error = crate::Config::from_args_with(
            vec!["--config".to_string(), "/tmp/upgone.json".to_string()],
            env(&[]),
            fs(&[]),
        )
        .expect_err("must fail");
        assert!(error.contains("/tmp/upgone.json"), "{error}");
    }

    #[test]
    fn an_absent_default_config_is_generated_before_it_is_read() {
        // ADR-0005: the first boot writes the whole default document —
        // secret included — and then resolves against what the file now
        // holds, so the operator edits the file the server actually read.
        let storage = fs(&[]);
        let config = crate::Config::from_args_with(Vec::<String>::new(), env(&[]), &storage)
            .expect("generated");
        assert_eq!(config.host, crate::DEFAULT_HOST);
        assert_eq!(config.port, crate::DEFAULT_PORT);
        assert_eq!(config.ttl_seconds, crate::DEFAULT_TTL_SECONDS);
        let contents = storage(&std::path::PathBuf::from(&config.config_path), None)
            .expect("read back")
            .expect("generated");
        let document: serde_json::Value = serde_json::from_str(&contents).expect("valid json");
        let secret = document["auth"]["jwt_secret"].as_str().expect("secret");
        assert_eq!(secret.len(), 64);
        assert!(secret.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(config.signing_secret.hex(), secret);
        assert!(contents.ends_with('\n'));

        // A second boot reads the same file, so both resolve one secret.
        let again = crate::Config::from_args_with(Vec::<String>::new(), env(&[]), &storage)
            .expect("second boot");
        assert_eq!(again.signing_secret.hex(), config.signing_secret.hex());
    }

    #[test]
    fn a_present_file_without_a_signing_secret_fails_naming_the_key() {
        let error = crate::Config::from_args_with(
            vec!["--config".to_string(), "/tmp/upgrade.json".to_string()],
            env(&[]),
            fs(&[("/tmp/upgrade.json", r#"{ "server": { "port": 9300 } }"#)]),
        )
        .expect_err("must fail");
        assert!(error.contains("jwt_secret"), "{error}");
    }

    #[test]
    fn an_empty_or_too_short_signing_secret_fails_too() {
        for secret in ["", "abcd"] {
            let error = crate::Config::from_args_with(
                vec!["--config".to_string(), "/tmp/uphalf.json".to_string()],
                env(&[]),
                fs(&[(
                    "/tmp/uphalf.json",
                    &format!(r#"{{ "auth": {{ "jwt_secret": "{secret}" }} }}"#),
                )]),
            )
            .expect_err("must fail");
            assert!(error.contains("jwt_secret"), "{secret:?} produced {error}");
        }
    }

    #[test]
    fn replacing_the_secret_changes_what_the_server_signs_with() {
        // The rotation story (ADR-0004): the file is the whole ceremony.
        let old = r#"{ "auth": { "jwt_secret": "abababababababababababababababababababababababababababababababab" } }"#;
        let new = r#"{ "auth": { "jwt_secret": "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd" } }"#;
        let before = crate::Config::from_args_with(
            vec!["--config".to_string(), "/tmp/urotate.json".to_string()],
            env(&[]),
            fs(&[("/tmp/urotate.json", old)]),
        )
        .expect("before");
        let after = crate::Config::from_args_with(
            vec!["--config".to_string(), "/tmp/urotate.json".to_string()],
            env(&[]),
            fs(&[("/tmp/urotate.json", new)]),
        )
        .expect("after");
        assert_ne!(before.signing_secret.hex(), after.signing_secret.hex());
    }

    #[test]
    fn the_ttl_default_and_the_ttl_key_are_both_honest() {
        let base =
            "\"jwt_secret\": \"abababababababababababababababababababababababababababababababab\"";
        let without = crate::Config::from_args_with(
            vec!["--config".to_string(), "/tmp/upttl1.json".to_string()],
            env(&[]),
            fs(&[(
                "/tmp/upttl1.json",
                &format!(r#"{{ "auth": {{ {base} }} }}"#),
            )]),
        )
        .expect("default ttl");
        assert_eq!(without.ttl_seconds, 86400);

        let with = crate::Config::from_args_with(
            vec!["--config".to_string(), "/tmp/upttl2.json".to_string()],
            env(&[]),
            fs(&[(
                "/tmp/upttl2.json",
                &format!(r#"{{ "auth": {{ {base}, "ttl_seconds": 3600 }} }}"#),
            )]),
        )
        .expect("configured ttl");
        assert_eq!(with.ttl_seconds, 3600);

        let zero = crate::Config::from_args_with(
            vec!["--config".to_string(), "/tmp/upttl3.json".to_string()],
            env(&[]),
            fs(&[(
                "/tmp/upttl3.json",
                &format!(r#"{{ "auth": {{ {base}, "ttl_seconds": 0 }} }}"#),
            )]),
        )
        .expect_err("zero ttl must fail");
        assert!(zero.contains("ttl_seconds"), "{zero}");
    }

    #[test]
    fn provider_entries_parse_now_and_wait_for_task_seven_to_be_validated() {
        let config = crate::Config::from_args_with(
            vec!["--config".to_string(), "/tmp/upprov.json".to_string()],
            env(&[]),
            fs(&[(
                "/tmp/upprov.json",
                r#"{ "auth": { "jwt_secret": "abababababababababababababababababababababababababababababababab", "oidc_providers": [ { "name": "work", "issuer": "https://id.example", "client_id": "upland" } ] } }"#,
            )]),
        )
        .expect("providers");
        assert_eq!(config.oidc_providers.len(), 1);
        assert_eq!(config.oidc_providers[0].name, "work");
        assert_eq!(config.oidc_providers[0].issuer, "https://id.example");
    }

    #[test]
    fn provider_names_are_validated_at_the_boot_that_reads_them() {
        let secret =
            "\"jwt_secret\": \"abababababababababababababababababababababababababababababababab\"";
        let entry = |name: &str| {
            format!(
                "{{ \"auth\": {{ {secret}, \"oidc_providers\": [ {{ \"name\": \"{name}\", \"issuer\": \"https://e\", \"client_id\": \"c\" }} ] }} }}"
            )
        };
        let cases = [
            ("Bad Name", "invalid provider name"),
            (&"a".repeat(33), "invalid provider name"),
            ("", "invalid provider name"),
        ];
        for (name, needle) in cases {
            let error = crate::Config::from_args_with(
                vec!["--config".to_string(), "/tmp/upname.json".to_string()],
                env(&[]),
                fs(&[("/tmp/upname.json", &entry(name))]),
            )
            .expect_err("must fail");
            assert!(error.contains(needle), "{name:?} produced {error}");
        }

        let duplicated = format!(
            "{{ \"auth\": {{ {secret}, \"oidc_providers\": [ \
             {{ \"name\": \"same\", \"issuer\": \"https://one\", \"client_id\": \"c\" }}, \
             {{ \"name\": \"same\", \"issuer\": \"https://two\", \"client_id\": \"c\" }} ] }} }}"
        );
        let error = crate::Config::from_args_with(
            vec!["--config".to_string(), "/tmp/updup.json".to_string()],
            env(&[]),
            fs(&[("/tmp/updup.json", &duplicated)]),
        )
        .expect_err("must fail");
        assert!(error.contains("duplicate provider name"), "{error}");
    }

    #[test]
    fn a_config_never_prints_its_signing_secret() {
        let config = crate::Config::from_args_with(
            vec!["--config".to_string(), "/tmp/updebug.json".to_string()],
            env(&[]),
            fs(&[(
                "/tmp/updebug.json",
                r#"{ "auth": { "jwt_secret": "abababababababababababababababababababababababababababababababab" } }"#,
            )]),
        )
        .expect("config");
        let text = format!("{config:?}");
        assert!(text.contains("SigningSecret"), "{text}");
        assert!(!text.contains("abab"), "{text}");
    }

    #[test]
    fn an_empty_config_path_is_a_failure_not_a_nudge() {
        let error = crate::Config::from_args_with(
            Vec::<String>::new(),
            env(&[("UPLAND_CONFIG", "")]),
            fs(&[]),
        )
        .expect_err("must fail");
        assert!(error.contains("config"), "{error}");
    }

    #[test]
    fn a_blank_host_in_the_file_is_a_failure_naming_the_key() {
        let error = crate::Config::from_args_with(
            vec!["--config".to_string(), "/tmp/upblank.json".to_string()],
            env(&[]),
            fs(&[("/tmp/upblank.json", r#"{ "server": { "host": " " } }"#)]),
        )
        .expect_err("must fail");
        assert!(error.contains("host"), "{error}");
    }

    #[test]
    fn a_port_out_of_range_in_the_file_fails_naming_the_port() {
        let error = crate::Config::from_args_with(
            vec!["--config".to_string(), "/tmp/upport.json".to_string()],
            env(&[]),
            fs(&[("/tmp/upport.json", r#"{ "server": { "port": 70000 } }"#)]),
        )
        .expect_err("must fail");
        assert!(error.contains("port"), "{error}");
    }

    #[test]
    fn the_default_config_path_shares_the_per_user_data_directory() {
        let config = crate::Config::from_args_with(Vec::<String>::new(), env(&[]), fs(&[]))
            .expect("defaults");
        #[cfg(target_os = "macos")]
        let expected =
            std::path::Path::new("/home/tester/Library/Application Support/upland/config.json");
        #[cfg(target_os = "windows")]
        let expected =
            std::path::Path::new("C:\\Users\\tester\\AppData\\Roaming\\upland\\config.json");
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let expected = std::path::Path::new("/home/tester/.local/share/upland/config.json");
        assert_eq!(config.config_path, expected);
    }

    #[test]
    fn the_config_file_is_replaced_by_a_flag_not_merged_around_it() {
        // --db-path says "this database"; the file's db_path must not
        // reopen underneath the operator's explicit word.
        let config = crate::Config::from_args_with(
            vec![
                "--db-path".to_string(),
                "/tmp/flagged.db".to_string(),
                "--config".to_string(),
                "/tmp/upflag.json".to_string(),
            ],
            env(&[]),
            fs(&[(
                "/tmp/upflag.json",
                r#"{ "server": { "db_path": "/tmp/file.db" }, "auth": { "jwt_secret": "abababababababababababababababababababababababababababababababab" } }"#,
            )]),
        )
        .expect("flag");
        assert_eq!(config.db_path, std::path::Path::new("/tmp/flagged.db"));
    }

    fn fs(
        entries: &[(&str, &str)],
    ) -> impl Fn(&std::path::Path, Option<&str>) -> Result<Option<String>, String> {
        let map = std::cell::RefCell::new(
            entries
                .iter()
                .map(|(path, contents)| (path.to_string(), contents.to_string()))
                .collect::<std::collections::HashMap<String, String>>(),
        );
        move |path, write| {
            let key = path.display().to_string();
            let mut map = map.borrow_mut();
            if let Some(contents) = write {
                map.entry(key.clone())
                    .or_insert_with(|| contents.to_owned());
            }
            Ok(map.get(&key).cloned())
        }
    }

    fn env<'a>(pairs: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_owned())
                .or_else(|| match key {
                    // Config::from_args resolves a default database path from
                    // the environment, and these tests mostly assert about
                    // other keys; the launcher variable for the current
                    // platform is present unless a test overrides it.
                    #[cfg(target_os = "windows")]
                    "APPDATA" => Some("C:\\Users\\tester\\AppData\\Roaming".to_owned()),
                    #[cfg(not(target_os = "windows"))]
                    "HOME" => Some("/home/tester".to_owned()),
                    _ => None,
                })
        }
    }

    async fn test_state() -> crate::AppState {
        crate::AppState {
            db: std::sync::Arc::new(
                crate::db::Db::in_memory()
                    .await
                    .expect("in-memory test database"),
            ),
            access_tokens: crate::auth::AccessTokenIssuer::new(
                crate::auth::SigningSecret::from_hex(
                    "abababababababababababababababababababababababababababababababab",
                )
                .expect("test secret"),
                3600,
            ),
            oidc_providers: vec![],
            id_tokens: crate::oidc::IdTokenValidator::Fixed(Default::default()),
        }
    }
}

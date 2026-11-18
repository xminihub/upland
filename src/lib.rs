use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::http::StatusCode;
use axum::http::Uri;
use axum::http::header::CONTENT_TYPE;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use rust_embed::RustEmbed;
use serde_json::{Value, json};

pub mod db;

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
}

pub fn router(state: AppState) -> Router {
    let router = Router::<AppState>::new()
        .nest("/api", api())
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

fn api() -> Router<AppState> {
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
}

/// The per-user data directory a platform expects a server's state in. No
/// `directories` dependency: three known paths, each resolved from
/// environment a launcher already sets.
fn default_db_path(env: &impl Fn(&str) -> Option<String>) -> Result<PathBuf, String> {
    let file_name = "upland.db";
    #[cfg(target_os = "macos")]
    let data_dir = env("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(target_os = "windows")]
    let data_dir = env("APPDATA").map(PathBuf::from);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let data_dir = env("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|home| PathBuf::from(home).join(".local/share")));

    let data_dir = data_dir.filter(|dir| !dir.as_os_str().is_empty());
    match data_dir {
        Some(dir) => Ok(dir.join("upland").join(file_name)),
        None => Err(
            "cannot resolve a default database path: no data directory in the environment; set --db-path or UPLAND_DB_PATH"
                .to_owned(),
        ),
    }
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

Environment:
  UPLAND_HOST, UPLAND_PORT, UPLAND_DB_PATH
      Defaults for --host, --port and --db-path; the flags win.
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

fn bind_and_storage<I, S, E>(args: I, env: &E) -> Result<(String, u16, PathBuf), String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
    E: Fn(&str) -> Option<String>,
{
    let args: Vec<String> = args.into_iter().map(Into::into).collect();
    let mut host: Option<String> = None;
    let mut port: Option<u16> = None;
    let mut db_path: Option<String> = None;

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
            other => return Err(format!("unknown option: {other}")),
        }
    }

    let host = host
        .or_else(|| env("UPLAND_HOST"))
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
            None => DEFAULT_PORT,
        },
    };
    // An empty UPLAND_DB_PATH is a failure rather than a nudge back to the
    // default: a unit file that lost its path
    // mid-edit would otherwise open a database nobody pointed it at, in a
    // directory nobody chose.
    let db_path = match db_path.or_else(|| env("UPLAND_DB_PATH")) {
        Some(value) if value.trim().is_empty() => {
            return Err("not a database path: the value must not be empty".to_owned());
        }
        Some(value) => PathBuf::from(value),
        None => default_db_path(env)?,
    };

    Ok((host, port, db_path))
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
        let (host, port, db_path) = bind_and_storage(args, &env)?;
        Ok(Self {
            host,
            port,
            db_path,
        })
    }
}

pub fn parse_args<I, S, E>(args: I, env: E) -> Result<Invocation, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
    E: Fn(&str) -> Option<String>,
{
    let args: Vec<String> = args.into_iter().map(Into::into).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        return Ok(Invocation::Version);
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(Invocation::Help);
    }
    Config::from_args(args, &env).map(Invocation::Serve)
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
        let config = crate::Config::from_args(Vec::<String>::new(), env(&[])).expect("defaults");
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

        let config = crate::Config::from_args(Vec::<String>::new(), env(&[])).expect("defaults");
        assert_eq!(config.db_path, expected);
    }

    #[test]
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn xdg_data_home_wins_over_the_local_share_fallback() {
        let config = crate::Config::from_args(
            Vec::<String>::new(),
            env(&[("XDG_DATA_HOME", "/srv/upland-data")]),
        )
        .expect("xdg");
        assert_eq!(
            config.db_path,
            std::path::Path::new("/srv/upland-data/upland/upland.db")
        );
    }

    #[test]
    fn the_database_path_flag_and_environment_are_honoured() {
        let from_flag = crate::Config::from_args(
            vec!["--db-path".to_string(), "/tmp/flag.db".to_string()],
            env(&[("UPLAND_DB_PATH", "/tmp/env.db")]),
        )
        .expect("flag");
        assert_eq!(from_flag.db_path, std::path::Path::new("/tmp/flag.db"));

        let from_env = crate::Config::from_args(
            Vec::<String>::new(),
            env(&[("UPLAND_DB_PATH", "/tmp/env.db")]),
        )
        .expect("env");
        assert_eq!(from_env.db_path, std::path::Path::new("/tmp/env.db"));

        // An empty UPLAND_DB_PATH is a failure, not a quiet default: the
        // operator wrote a path here and losing it silently is how the wrong
        // database gets opened.
        let error = crate::Config::from_args(Vec::<String>::new(), env(&[("UPLAND_DB_PATH", "")]))
            .expect_err("must fail");
        assert!(error.contains("database path"), "{error}");
    }

    #[test]
    fn host_and_port_flags_are_honoured() {
        let config = crate::Config::from_args(
            vec![
                "--host".to_string(),
                "0.0.0.0".to_string(),
                "--port".to_string(),
                "9000".to_string(),
            ],
            env(&[]),
        )
        .expect("flags");
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 9000);
    }

    #[test]
    fn flags_come_before_the_order_they_are_written_in() {
        let config = crate::Config::from_args(
            vec![
                "--port".to_string(),
                "9001".to_string(),
                "--host".to_string(),
                "::1".to_string(),
            ],
            env(&[]),
        )
        .expect("flags");
        assert_eq!(config.host, "::1");
        assert_eq!(config.port, 9001);
    }

    #[test]
    fn environment_variables_apply_when_the_flags_are_absent() {
        let config = crate::Config::from_args(
            Vec::<String>::new(),
            env(&[("UPLAND_HOST", "0.0.0.0"), ("UPLAND_PORT", "9100")]),
        )
        .expect("env");
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 9100);
    }

    #[test]
    fn a_flag_wins_over_the_environment_variable_it_disagrees_with() {
        let config = crate::Config::from_args(
            vec!["--port".to_string(), "7000".to_string()],
            env(&[("UPLAND_HOST", "0.0.0.0"), ("UPLAND_PORT", "9100")]),
        )
        .expect("flag beats env");
        assert_eq!(config.port, 7000);
        assert_eq!(config.host, "0.0.0.0");
    }

    #[test]
    fn an_unusable_port_is_a_failure_and_not_a_default() {
        for port in ["not-a-number", "0", "65536"] {
            let error =
                crate::Config::from_args(vec!["--port".to_string(), port.to_string()], env(&[]))
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
            let error =
                crate::Config::from_args(Vec::<String>::new(), env(&[("UPLAND_PORT", value)]))
                    .expect_err("must fail");
            assert!(error.contains("port"), "{value} produced {error}");
        }
    }

    #[test]
    fn an_ipv6_listen_address_is_bracketed() {
        // "::1:8080" is itself a legal IPv6 literal meaning something else, so
        // an unbracketed IPv6 host would bind an address nobody wrote down.
        let config = crate::Config::from_args(
            vec![
                "--host".to_string(),
                "::1".to_string(),
                "--port".to_string(),
                "9100".to_string(),
            ],
            env(&[]),
        )
        .expect("ipv6 loopback");
        assert_eq!(config.listen_address(), "[::1]:9100");

        let ipv4 =
            crate::Config::from_args(Vec::<String>::new(), env(&[("UPLAND_HOST", "0.0.0.0")]))
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
            let error = crate::Config::from_args(args.clone(), env(&[])).expect_err("must fail");
            assert!(!error.is_empty(), "{args:?} produced no message");
        }
    }

    #[test]
    fn an_unknown_option_is_a_failure_naming_it() {
        let error =
            crate::Config::from_args(vec!["--addr".to_string(), "0.0.0.0".to_string()], env(&[]))
                .expect_err("must fail");
        assert_eq!(error, "unknown option: --addr");
    }

    #[test]
    fn an_empty_host_never_reaches_the_listener() {
        let from_flag =
            crate::Config::from_args(vec!["--host".to_string(), " ".to_string()], env(&[]))
                .expect_err("a blank host is not an address");
        assert!(from_flag.contains("host"), "{from_flag}");

        let from_env = crate::Config::from_args(Vec::<String>::new(), env(&[("UPLAND_HOST", "")]))
            .expect_err("an empty host is not an address");
        assert!(from_env.contains("host"), "{from_env}");
    }

    #[test]
    fn version_and_help_never_start_a_server() {
        // Formula/upland.rb's `test do` runs `upland --version` and matches its
        // own pinned version against the output, so --version must be a print,
        // and it must be reached even when the rest of the command line is bad.
        let version = crate::parse_args(vec!["--version".to_string()], env(&[])).expect("version");
        assert!(matches!(version, crate::Invocation::Version));
        let short = crate::parse_args(vec!["-V".to_string()], env(&[])).expect("short version");
        assert!(matches!(short, crate::Invocation::Version));

        let help = crate::parse_args(vec!["--help".to_string()], env(&[])).expect("help");
        assert!(matches!(help, crate::Invocation::Help));
        let short_help = crate::parse_args(vec!["-h".to_string()], env(&[])).expect("short help");
        assert!(matches!(short_help, crate::Invocation::Help));

        let bad = crate::parse_args(
            vec![
                "--version".to_string(),
                "--port".to_string(),
                "nope".to_string(),
            ],
            env(&[]),
        )
        .expect("version beats a broken port");
        assert!(matches!(bad, crate::Invocation::Version));
    }

    #[test]
    fn serving_is_the_default_invocation() {
        let serve = crate::parse_args(vec!["--port".to_string(), "9100".to_string()], env(&[]))
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
            "--version",
            "--help",
            "UPLAND_PORT",
            "UPLAND_DB_PATH",
            "127.0.0.1",
        ] {
            assert!(usage.contains(expected), "usage omits {expected}");
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
        }
    }
}

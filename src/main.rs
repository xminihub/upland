use std::env;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let config = match upland::parse_args(env::args().skip(1), |key| env::var(key).ok()) {
        Ok(upland::Invocation::Serve(config)) => config,
        Ok(upland::Invocation::Version) => {
            println!("{}", upland::version_line());
            return ExitCode::SUCCESS;
        }
        Ok(upland::Invocation::Help) => {
            print!("{}", upland::usage());
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("upland: {error}");
            eprint!("\n{}", upland::usage());
            return ExitCode::from(2);
        }
    };

    let addr = config.listen_address();

    // Open the database before binding: a machine that cannot write its data
    // directory should fail without having taken a port first.
    let db = match upland::db::Db::connect(&config.db_path).await {
        Ok(db) => std::sync::Arc::new(db),
        Err(error) => {
            eprintln!(
                "upland: cannot open the database at {}: {error}",
                config.db_path.display()
            );
            return ExitCode::FAILURE;
        }
    };

    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("upland: cannot listen on {addr}: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!("upland listening on http://{addr}");
    println!("upland database at {}", db.path().display());
    let state = upland::AppState {
        db,
        access_tokens: upland::auth::AccessTokenIssuer::new(
            config.signing_secret,
            config.ttl_seconds,
        ),
        oidc_providers: config.oidc_providers,
        id_tokens: upland::oidc::IdTokenValidator::Live,
    };
    axum::serve(listener, upland::router(state))
        .await
        .expect("server should run");

    ExitCode::SUCCESS
}

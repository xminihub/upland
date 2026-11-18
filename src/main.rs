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

    let addr = format!("{}:{}", config.host, config.port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("upland: cannot listen on {addr}: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!("upland listening on http://{addr}");
    axum::serve(listener, upland::router())
        .await
        .expect("server should run");

    ExitCode::SUCCESS
}

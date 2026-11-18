# 2. Loopback by default, and the CLI surface

Date: 2026-11-18

## Context

`src/main.rs` was eight lines: bind `0.0.0.0:8080`, serve. It read no arguments,
so there was no way to run two uplands, no way to move off a port a browser
extension or a dev server already held, and no way to ask an installed binary
what version it was.

That was tolerable while the only way to run upland was `cargo run` on a laptop.
It stopped being tolerable the moment Homebrew entered the picture
(ADR-0001): the tap's formula template proves an install by running
`<bin> --version`, and a formula for a server is expected to declare a
`service` block, which means `brew services start upland` runs whatever the
binary's defaults are — on a machine where nobody typed anything, in a job that
restarts itself, on a port nobody chose. `0.0.0.0` in that position means the
first thing a person does after `brew install upland` is publish this
repository's JSON API to their LAN.

The other half of the exposure was invisible in the code: the SPA fallback makes
`GET /` and every unknown path serve the embedded UI, so an internet-facing
bind is not an API-only risk.

## Decision

- The **listen address** defaults to `127.0.0.1:8080`. Binding every interface
  is a thing a person or a deployment asks for, in one of two visible ways:
  `--host 0.0.0.0` on the command line, or `UPLAND_HOST=0.0.0.0` in the
  environment of whatever starts it.
- The surface is `--host <addr>`, `--port <port>`, `--version`/`-V`,
  `--help`/`-h`, with `UPLAND_HOST` and `UPLAND_PORT` as defaults for the two
  flags. A flag beats the environment variable it disagrees with, because
  `brew services` and a systemd unit put the environment in a file a person
  will never open, and the command line is where an operator says "not today".
- `--version` prints `upland <version>` from `CARGO_PKG_VERSION` and exits
  successfully before anything is bound — even when the rest of the command
  line is nonsense. That string is what `Formula/upland.rb`'s `test do` matches
  against its own pinned version, so the release tag, the formula and the
  binary have one source: the version in `Cargo.toml`.
- `--help` prints the flags, the defaults and the two environment variables.
- Bad input is an error, not a fallback: an unknown option, a `--port` that is
  not 1–65535, or a `--port`/`--host` missing its value prints
  `upland: <what went wrong>`, then the usage, and exits `2`. A blank host is
  rejected; an unparseable `UPLAND_PORT` is ignored in favour of the default,
  because that variable is set by whoever launched us and a stale shell export
  is not a reason to refuse to start.
- Parsing lives in `src/lib.rs` (`Config::from_args`, `parse_args`,
  `version_line`, `usage`) with the unit tests in the same file, and
  `src/main.rs` only maps the result to output and exit codes. No socket is
  opened by any test, in keeping with `tests/api.rs` and `tests/spa.rs`.
- No new dependency. upland's tree is axum, tokio, serde_json, rust-embed;
  four flags and two environment variables do not justify a clap dependency,
  its derive macros and the `clippy -Dwarnings` surface that comes with them.
  `rust-embed` and the router are unchanged.

## Consequences

- Deploying upland somewhere it must be reachable from another machine now
  requires saying so — in a container's `args`, in a systemd unit's
  `Environment=`, or in a reverse proxy's upstream. That is the intent: a
  default that a forgotten `brew services start` cannot turn into an open port.
- `cargo run` binds loopback, so `README.md`, `AGENTS.md` and anything pointing
  at `localhost:8080` keep working, and a phone on the same Wi-Fi no longer
  reaches a developer's server.
- `brew test upland` and the release's per-target smoke step (`upland --version`
  on the artifact it just built) became possible, which is how ADR-0001's
  "compiled is not the same as runs" gap gets closed without an installer.
- The tap's service block passes `--host` and `--port` explicitly instead of
  relying on these defaults, so a future change here cannot silently move a
  running service.
- Two names, `--host` and `--port`, are now load-bearing across repositories:
  the formula's `service` and `test do` blocks both use them. Renaming a flag
  is a two-repository change, discovered at the tap's bump, not here.
- Graceful shutdown, signals and `--bind`-style aliases are untouched: this
  decision is about who may reach the socket, not how it closes.

## Rejected alternatives

- **Keep `0.0.0.0` and add only `--port`.** The smallest change that satisfies
  `brew test`, and it leaves `brew services start upland` publishing an API to
  the network on a machine the user thinks of as theirs.
- **Keep `0.0.0.0`, document the risk.** A caveat nobody reads is the same
  decision made quietly.
- **Environment variables only, no flags.** Launchd and systemd can set an
  environment, but a person debugging on a laptop cannot override one without
  editing a generated file.
- **A config file.** A format is a promise about persistence, error handling
  and precedence, and the wayfinder map's persistence decisions are still open.
- **clap.** Better `--help` and free `--version`, for a four-flag surface and a
  dependency tree in every build of a server binary.
- **`--addr`/`--listen` as one flag.** `0.0.0.0:8080` is one string to parse and
  one more error case; two flags map onto the two environment variables and
  onto what a `service` block needs to say.
- **A `serve` subcommand, with `--version` at the top level.** There is exactly
  one verb; subcommands would be a shape to keep for a second one that is not
  planned.

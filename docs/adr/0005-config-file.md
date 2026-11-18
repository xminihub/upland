# 5. The config file, superseding a rejection

Date: 2026-11-18

## Context

ADR-0002 rejected a config file as a decision surface: "a format is a
promise about persistence, error handling and precedence, and the wayfinder
map's persistence decisions are still open." That rejection was honest at
the time — the only knobs were host, port and database path, all carryable
by three flags and three environment variables.

Authentication (ADR-0004) changed the arithmetic. An OIDC provider registry
is a *list of structured records* — name, issuer, client id — and an
environment variable is a terrible home for a list: no encoding a person
would hand-write in a launchd plist survives review. The signing secret and
the token lifetime are boot-time config with failure semantics that belong
next to the registry's. The fog ADR-0002 pointed at has, for these objects,
lifted: the persistence question is answered (a file on disk beside the
**database**, in the per-user data directory of ADR-0003), and this ADR now
makes the format promise the rejection said was missing.

## Decision

- **The config file is JSON** — `upland/config.json` by default, in the same
  per-user data directory as the **database**, pointed elsewhere by
  `--config <path>` or `UPLAND_CONFIG`. JSON because `serde_json` is already
  load-bearing in this tree and a second parser format (TOML, YAML) is a
  second dependency plus a second set of error modes. The cost is admitted:
  no comments, in the one file whose content invites "why is this here".
- **It holds everything.** `server { host, port, db_path }` joins
  `auth { jwt_secret, ttl_seconds, oidc_providers[] }`. The existing flags
  and environment variables keep working unchanged, with precedence:
  **flag > environment > file > built-in default**. ADR-0002's documented
  surface is a strict overlay on this file, never a contradiction of it.
- **Strict parsing is a boot failure, naming the path.** An unknown key
  anywhere in the document, a malformed file, or a present-but-secretless
  config all print what is wrong and exit — code 2, the bad-flag path's
  existing behaviour. A silently ignored typo (`oidc_provideers`) means an
  issuer nobody configured, and that is worse than a crashed service:
  loud at boot, per ADR-0002's own doctrine.
- **First boot generates the file rather than failing without it.** A fresh
  install — the brew service, the `cargo run` on a laptop — writes the full
  default document (defaults in `server`, `ttl_seconds` 86400, an empty
  provider list, a random 32-byte hex `jwt_secret`) at mode `0600`, with an
  atomic write so two racing boots cannot tear it. Generation is the
  *default location's* right: a path the operator named with `--config` or
  `UPLAND_CONFIG` must exist, because a mistyped named path is the same
  silent-misconfiguration this file's strict parsing exists to refuse, and
  generating there would paper over it. The operator then owns
  the file: replacing the secret and restarting is the revocation story
  ADR-0004 has no table for.
- **Provider records carry no client secret.** The OIDC flow is browser-side
  PKCE with the backend as verifier (ADR-0004), so each entry is exactly
  `{ name, issuer, client_id }`: name matching `[a-z0-9-]{1,32}`, unique in
  the file, because it is what API payloads and the login screen will quote
  forever. Secrets belong to the issuer conversation the browser has, not
  to this file.

## Consequences

- ADR-0002's Rejected-alternatives line "A config file" is **superseded**
  by this ADR, and ADR-0002's Decision list is otherwise untouched: the
  flags, the environment variables, and their precedence stand exactly as
  written, now as the third of four layers.
- The format promise is now real: adding a key is a schema change with an
  error-handling and precedence story, and removing one is a breaking
  change for hand-edited files. This is the price ADR-0002 said would have
  to be paid before a file was worth having; authentication is what pays it.
- The file holds the signing secret, so it lives and dies by the per-user
  data directory's permissions — the same trust domain as the database
  file. `0600` on generation; operators who loosen it own the result.
- `--help` and the README now have to describe four layers of precedence.
  The usage text gains `--config`/`UPLAND_CONFIG`; nothing existing moves.
- Future subsystems (logs, analytics — the remaining reserved tables) get a
  place to live in this document rather than inventing their own surface.

## Rejected alternatives

- **TOML.** The Cargo-ecosystem familiarity is real, and the no-comments
  complaint cuts the other way against JSON; but it is a new parser
  dependency in a tree that pruned one crate before. Rejected.
- **YAML.** Deploy-familiar, but an indentation-error class of its own and a
  maintenance-fading parser crate. Rejected.
- **Environment variables only.** Fine for four scalars, hopeless for a
  list of records. This is exactly what the registry made untenable.
  Rejected.
- **Flags + env with an override hook for secrets only.** Keeps ADR-0002's
  two-layer shape but splits the OIDC story across a flag, an env var and a
  list that needs a file anyway. Inconsistent for no gain. Rejected.
- **A required `--config`:** every path explicit, but plain `cargo run` and
  the tap's service block both gain mandatory ceremony the generated
  default file makes unnecessary. Rejected.
- **A config table in the database.** One less file format, but the boot
  that reads it needs the database path from it — a chicken-and-egg the
  file resolves — and secrets would inherit a world-readable default where
  the filesystem offers `0600`. Rejected.

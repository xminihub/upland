# 4. Authentication: stateless JWT, two doors, one email key

Date: 2026-11-18

## Context

Upland served its JSON API to whoever could reach the **listen address**.
There was no notion of a person: no account, no sign-in, no way to tell one
caller from another, and every route — the SPA bundle's data included — was
anonymous. ADR-0002 made loopback the default, which protects a laptop, but
the operator can bind every interface, and the distribution story
(ADR-0001's brew service) runs the server in a job nobody is watching.

The first real feature is therefore **authentication**, with two doors:
**username/password** and **OIDC**. The decisions below came from a grilling
session that walked the whole design tree; the spec is
[#13](https://github.com/xminihub/upland/issues/13), the work is tasks #14–#25.

## Decision

- **Multi-user, open signup.** Anyone who can reach the server can create an
  account. Exposure is the operator's concern (the listen address), not the
  registration door's.
- **An account is a username and an email**, both stored lowercased and
  unique, and an optional **credential**: the bcrypt hash (cost 12) lives
  only in the `credentials` table, ADR-0003's reserved word for it kept
  honest — an account created through OIDC simply has no credential row.
  Each OIDC sign-in attaches an **identity**, an (issuer, subject) pair,
  unique per pair.
- **Email is the linking key, verified or not.** An OIDC login whose email
  matches an existing account lands in that account regardless of the
  provider's `email_verified` claim; a novel email auto-creates an account
  whose username is derived from the local part, collision-suffixed. A
  provider that asserts no email is refused outright. The configured
  **provider** is the operator's trust decision, and this decision loads
  that trust.
- **Bearer-only.** No cookies, anywhere. The **access token** is the only
  credential: the SPA keeps it in web storage and sends
  `Authorization: Bearer` on every call; scripted clients do the same. CSRF
  leaves the design because cookies left it.
- **Stateless HS256 JWT, minimal claims.** `iss: "upland"`, `aud: "upland"`
  (the audience check is what stops symmetric-key tokens crossing audit
  boundaries), `sub` (the account id), `iat`, `exp`. Lifetime is a config
  key, default 24 hours. No sessions table, no refresh tokens, no
  server-side revocation: **logout is a documented no-op** that tells
  clients where to forget the token, and revocation is replacing the
  **signing secret** and restarting.
- **The signing secret is generated, then operator-owned.** First boot writes
  the **config file** (ADR-0005) with a random 32-byte hex secret at mode
  `0600`; a config that exists with a missing or empty secret fails the boot
  naming the key. Rotation is an edit, not a ceremony.
- **OIDC is SPA-side PKCE; the backend is a verifier, not a confidential
  client.** The browser runs discovery and the code exchange against the
  issuer directly and hands the resulting ID token to the API, which
  validates signature, issuer, audience and expiry with the `openidconnect`
  crate and mints the upland token. No client secret is stored server-side
  for any provider. Multiple providers are configured concurrently.
- **Every `/api/v1` route requires a token** except the named exempt set —
  signup, login, the OIDC exchange, and the provider listing the login
  screen needs before anyone has credentials. `/healthz` stays unversioned
  and public; the SPA shell is static assets and stays open because the
  data behind it is not.
- **Field rules are minimal and explicit**: username 3–32 of `[a-z0-9._-]`;
  email trimmed, lowercased, "one `@`, text both sides" — claimed, not
  verified (there is no mailer to verify with); passwords over bcrypt's
  72-byte truncation point are rejected rather than silently shortened.
- **Failures answer codes, not prose**: `{"error": {"code": "...", "field":
  "..."}}`, snake_case, stable across releases, so UI forms branch on the
  code instead of parsing English.
- **No throttling in feature one.** Loopback-by-default plus bcrypt's own
  cost is the bet; it gets revisited before anonymous exposure moves from
  the operator's choices into the product's promises.

## Consequences

- A stolen token is valid until it expires, and the server cannot shorten
  that: the stateless choice bought no-restart-revocation-free operation with
  real revocation's absence. The signing-secret edit is the escape hatch.
- Any XSS in the SPA can read the stored token — the accepted price of
  bearer-only uniformity between browser and scripted clients.
- A lax IdP that over-asserts emails can hand a password account's front door
  to a stranger. Mitigation is operator configuration, not code, and the
  README says so.
- Unthrottled bcrypt on a `0.0.0.0` bind is a free password-guessing surface
  and a cheap CPU amplifier. Documented, deliberate, scheduled for a later
  issue.
- The dependency tree grows: `serde` + derive (the codebase hand-rolled
  `serde_json` before), `bcrypt`, `jsonwebtoken`, `openidconnect` and what
  they pull. ADR-0002's no-new-dependency stance was about clap; a
  verifier that is correct by following the spec instead of by our tests is
  a different trade.
- `sessions` is retired from CONTEXT.md's reserved tables — the stateless
  decision made the word a lie — while `credentials` keeps its exact
  reserved meaning.
- Feature one stops at getting in: password change, email change, username
  rename, account delete, identity connect/disconnect are each their own
  future decisions, several of which (email change vs. the linking key)
  reopen this ADR's ground and must be argued on their own.

## Rejected alternatives

- **Opaque tokens with a sessions table.** Real logout and per-device
  revocation, and the natural resident of the reserved table — but it
  re-introduces server state this design chose to avoid, and a signing key
  it must keep secret anyway. Rejected.
- **JWT + refresh tokens.** Two formats, rotation races, and the stored
  state the stateless branch exists to refuse. Rejected.
- **Cookie sessions.** The browser/API same-origin shape invites it, but it
  forks the transport for scripted clients and drags CSRF back in.
  Rejected.
- **Argon2id.** OWASP's current first choice; bcrypt was picked for its
  fixed, boring cost and the 72-byte rule made an explicit reject instead
  of a silent truncate. Rejected, cheaply reversible.
- **Gating the link on `email_verified`.** The safer default — refused
  logins beat merged accounts — but providers disagree loudly about the
  claim, and the operator chose the issuer. Rejected by the owner of the
  trust, not the implementer.
- **Hand-rolled OIDC verification** (`jsonwebtoken` + fetched JWKS): a
  smaller tree, and every validation bug in this code would be one this
  feature shipped. Rejected.
- **A confidential-client flow with server-side code exchange.** Needs
  client secrets on disk and a token handed to the SPA through a URL
  fragment — exactly the browser-exposed credential bearer-only exists to
  avoid. Rejected.
- **Required operator secret at first boot.** Keeps "operator-supplied"
  literal, but crash-loops every fresh `brew services start`. Rejected in
  favour of generate-then-own.

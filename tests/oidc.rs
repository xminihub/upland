// The OIDC door, tested the agreed way (spec #13): recorded keys and
// hand-signed ID tokens injected through the client boundary the
// openidconnect verification crosses. The crate's crypto is exercised
// for real — signature, issuer, audience, expiry all reject like
// production would — and no test opens a socket.
//
// The two PEM fixtures below are throwaway RSA keys used only by this
// test file. They protect nothing; do not reuse them anywhere real.

use std::collections::HashMap;

use axum::http::StatusCode;
use axum_test::{TestResponse, TestServer};
use openidconnect::JsonWebKeySet;
use openidconnect::core::CoreJsonWebKey;
use serde_json::{Value, json};

use upland::OidcProvider;
use upland::oidc::IdTokenValidator;

const TEST_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDBREnV/6/m1KcH
2Ml6LfmS798sWrAsMn9X5UVz1nbFhu4NXsm7MqgQSPRmI4IoXbjFnLsVp4JGwiv6
GCWbsIu5V0kGWGz5ie4cAH8EZVdgFQEG2/qzWoHzpgKax0D3Egrx2wnnmvyKK2mK
17isGeRcX2/urCffYGXJpbB8MPH8rz3dS6dh2fmGduTurNr5mA6RrV/JiNALQqGl
kP7Zf9LNZlCya5PaVOpX9svSxBIHyO1n3BfqIL2uNClvJbfgp9x/rEh921oCIHG4
2Wud1bnLmEfdw0uzItw5u3B7SzeQ4kFZiopB11MfYhf7OuWueuTnRrIuoI1r/gjW
hfLx92I1AgMBAAECggEAKC4zoJ1qO3YqLNA2ISP2T4V102GeX233OjWlMR5dLpdt
HhP/Il93f/0+7I5E1PIZRGJfpfKfu+jKHlKPXRVBE10xhjNzopS5z5IN1kLgfc4Y
Gs44yrmOK3Lv9zBmUgerBVnp7wAysWIxrOdJEmTGrKq92XPqzT7cIqaDMOq3ILaM
aslDbv/3QuNidI6TGQL3JV0UtQDSeN/4BNHJmjgS1gFdjUeHnFqQhOWleh/8DI3s
hupkVW1P5scnovpF79Xkk64yOb3ZDwUfofDtBqlMPTq7d9pSUDL60nQivAgiUP49
+2r3b2qNv/5miCgdKvKpxkM55LWdxlc0+drVjRhgMQKBgQDzd2vwBiz03g39FTLD
5OWU28VxKn56m1PYqO2TBNmBFg4OMSzJWWqRN4kc1uyVpysRgwvJ6WRlfSCLqRwV
uoq4pXc91FgbkgnR4zOd2tJcGgNzvLH19co/tVpWIFrTjFNXEUlJClywfzS/4Hwh
haI7Df4gJ8u3pIk4VdKiWNjUBQKBgQDLN0xTF52puAZti8HSgpOhg0j8/QB4XJOt
H2HYedOmhZ8cVnPdxzlUH/qnBFglsMiaAn3r04I60GAG0YO1o9r31EY0863x4ZxT
gSUG6RZChSRdj9N3KgwK5Z7ozAotH9fNLinDJtxWeXNuFguXdJwrU9F9X92rtRwK
o2qBgtBccQKBgQDsxawD/o2ECdvCz/XRyu52yRCIENJ1/E8bepWpD+NTBZbfuHUc
z8eyuDaxBFeK5sscubwiPcc0Z+7Zw7mbbBHypoCDcdnJ3kmMhA+zqZZAmc/Xo5/S
Wpm5QW48KjUKXQLDvvztepVP1DYBkJhHpYVzNxV4Qht9WhdU4Sn6Vrtv3QKBgCeU
EY1tYAuaB8Z2Hb2+KLqgMyryq5DFlw10uz3KHGqmQ6LlQ5PCJWw4/jxhIDnXAHre
WWAAxbBmvQ0W6SwLSa5S7YmKnCor4qJw2KFL7EXwYHDIfksLKWVF63kzpBe9x+3y
O7gCbY95//1E1eUdlPzrSjW84hN7Job75NZsf+BBAoGBAM7iyWtkM5BIWJigS8A4
x+DdDr7VC3ptRpsRQHZcw1QhJ9/YbQXmkqWbfcuNZY//nB/J8MDh3j8NcgwJNcV6
v2IZGJJUc7TpM2hp7eh6n9rL3hFO9mq0XEBNEQ6rU6Ef6T2SXy3jXn8W9R6z9cB2
snrjcwzlKnh0Z0c0H+8gT8Yp
-----END PRIVATE KEY-----
";

const FOREIGN_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDRjGTFh02BaNM4
Pjlv7HeFv7+zjcA1xBFNKRyqcL6GGDg5SqUFwvBkeWD9tpqo7FXtWSGBVaPmtPA4
ePWo14SYhiEamVcIsJBkU/X//Xx+Bbip6prmkxqYrJpLYISofoiVBR1jMJQbv5F0
1k8nrFuR0I2Ibt/iyn3vfsLyO3SVd3s6yx4tzsR968KZ+Tg82O6zv0GoBgyDwqD1
kDePgOjZDr5PW1QQqaJT7ykHiLki+HZnmqA/VRwiOOuHw79pRpwPWgA8hMjLO70x
J5abTFLY3RPu38jmPx96yiSvD9r/VM5VuAthq3pFwNSBwEHXGTQJR0FqnjgypL7X
rFs51hcJAgMBAAECggEAFsuma0ATsAIgpyfKvQic/Htubeib2iV7Cdkw9YAoE8OW
+zGJ9/Fb0BxNrzACDWIPW290qAKSXs68arJK7BwM4EBIIeIAQDFf9dAV7OwNEtMJ
wo9DXFWwoR72toA4kjFYp5hruN6AIDAj8VE+gWb8O6Wzvd2s5JmpX0g7ihkqbrdj
NnH0g2ybgiHiFYm51Cg0qyic8Um1JD1JLFL5icAnF/BP57OTLlh3VAoFLjplGHtg
fpMIw5RYNscP0lR9AaNzBrxhMsfhR8Atf5CC6Et6R5Kbp3YRZ8djXlCy+YLcOWV4
lHZd+JaiQwhrSXzlZxO3rwDuvqulK3XLhVkTn9FrcQKBgQDq0HPQi8ELlqfBKIh7
pBjBnLmcR6/DI49j+GA7uXUv1jxluVsvbg6ld2ypMDbZBOWcUUXbFC6tH1ljDSgy
4b7510huvndBgzoY6djX2DJbb8SwcVuQbqw/S0AIVX4QBLrmipvRWDUmXsgIwpck
79WlONzjkU9pjOBdmnu9aNVVGQKBgQDkdF8IrS0ahgW2GjtQsYs0eCfru3BMwxIB
Fh69Tt+DKf3QQvbsa3/pWYvLzLA75Kh4sreqf5sWZBQ/JUsG8qvWFMZP/s5sPhV7
FWiuOX0vmZMmMFH8Y9BczQK4Ewjb7n9sn4ZM+I8x3DO2qu721UddHB0n7NYuR5UQ
JAK0c3mfcQKBgHeaB3OQy5pAeV/MaoA3nBh54Jy2ctL9ONsdtr6U/xiHkanqmIR3
FWS4iJNAqJ8vfFh7mQ42oHOzxQ7Ls87+5wxQ2Gb4ArxOv/fSnI2MZFHogxGwG4hc
psVyacIU8antDtnAfg9KU4AgxNiZbOz2cCbA6NnMuNs1U066Eg7u58J5AoGBAN2n
UvV2IIbVdgEjHl3ChQ+ZbRI4wFKaCYPoVFbUieQW/93quuzTuD1N1mcb5/mUdyj0
LWNNf8mPvF0v78WD+dkyOt8umFVJP0o2kWRI/v8KAE1TIJjCt0zy1CqIvVW15wgb
FxDjMPvtbHdVDZmLgTwDpP4xufUZGrWYWs9+IojhAoGALHmCCD7F+e9WWCzKkKrS
auPbIhHDdZxnWURNIGmvAM26C6OTQDc/fPj8BlMmKRB1zR0fG3E+V3GCZzfsmxGH
iDVBEyKDZuVwktl4csLHofCj74/RN55ltg34nobtEJ5LkhMx+vxtpbiSn77MHUww
bP+Ii8tKQbKnre/D97NRZK0=
-----END PRIVATE KEY-----
";

const ISSUER: &str = "https://issuer.invalid";
const CLIENT: &str = "upland-work";
const KID: &str = "upland-test";

fn base64url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TABLE[(n >> 18) as usize & 0x3f] as char);
        out.push(TABLE[(n >> 12) as usize & 0x3f] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(n >> 6) as usize & 0x3f] as char);
        }
        if chunk.len() > 2 {
            out.push(TABLE[n as usize & 0x3f] as char);
        }
    }
    out
}

fn published_jwks() -> JsonWebKeySet<CoreJsonWebKey> {
    use rsa::pkcs8::DecodePrivateKey;
    use rsa::traits::PublicKeyParts;

    let key = rsa::RsaPrivateKey::from_pkcs8_pem(TEST_KEY_PEM).expect("test key");
    let public = rsa::RsaPublicKey::from(&key);
    let document = json!({
        "keys": [{
            "kty": "RSA",
            "kid": KID,
            "use": "sig",
            "alg": "RS256",
            "n": base64url(&public.n().to_bytes_be()),
            "e": base64url(&public.e().to_bytes_be()),
        }],
    });
    serde_json::from_value(document).expect("jwks")
}

fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after 1970")
        .as_secs() as i64
}

/// Sign an ID token the way a configured issuer would — or the way an
/// attacker, a stale clock, or a second-rate IdP would, when the test
/// asks for it.
fn signed_token(
    signing_pem: &str,
    subject: &str,
    issuer: &str,
    audience: &str,
    expires_in: i64,
    email: EmailClaim,
) -> String {
    let now = now_seconds();
    let mut claims = json!({
        "iss": issuer,
        "aud": audience,
        "sub": subject,
        "iat": now,
        "exp": now + expires_in,
    });
    match email {
        EmailClaim::Absent => {}
        EmailClaim::Value(value) => {
            claims["email"] = json!(value);
        }
        EmailClaim::Unverified(value) => {
            claims["email"] = json!(value);
            claims["email_verified"] = json!(false);
        }
    }
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some(KID.to_owned());
    jsonwebtoken::encode(
        &header,
        &claims,
        &jsonwebtoken::EncodingKey::from_rsa_pem(signing_pem.as_bytes()).expect("signing key"),
    )
    .expect("sign")
}

enum EmailClaim {
    Absent,
    Value(&'static str),
    Unverified(&'static str),
}

fn provider() -> OidcProvider {
    OidcProvider {
        name: "work".to_owned(),
        issuer: ISSUER.to_owned(),
        client_id: CLIENT.to_owned(),
    }
}

async fn server_with(providers: Vec<OidcProvider>) -> TestServer {
    let db = upland::db::Db::in_memory()
        .await
        .expect("in-memory test database");
    let mut keys = HashMap::new();
    keys.insert("work".to_owned(), published_jwks());
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
        id_tokens: IdTokenValidator::Offline(keys),
    }))
}

async fn oidc_login(server: &TestServer, provider_name: &str, id_token: &str) -> TestResponse {
    server
        .post("/api/v1/auth/oidc")
        .json(&json!({ "provider": provider_name, "id_token": id_token }))
        .await
}

async fn signup(server: &TestServer, username: &str, email: &str) -> TestResponse {
    server
        .post("/api/v1/auth/signup")
        .json(&json!({ "username": username, "email": email, "password": "hunter2" }))
        .await
}

async fn me(server: &TestServer, token: &str) -> Value {
    server
        .get("/api/v1/auth/me")
        .add_header("authorization", format!("Bearer {token}"))
        .await
        .json::<Value>()
}

fn token_of(response: &TestResponse) -> String {
    response.json::<Value>()["access_token"]
        .as_str()
        .expect("access token")
        .to_owned()
}

#[tokio::test]
async fn the_provider_listing_is_readable_before_anyone_signs_in() {
    let server = server_with(vec![
        provider(),
        OidcProvider {
            name: "home".to_owned(),
            issuer: "https://accounts.example".to_owned(),
            client_id: "upland-home".to_owned(),
        },
    ])
    .await;
    let response = server.get("/api/v1/auth/providers").await;
    response.assert_status_ok();
    assert_eq!(
        response.json::<Value>(),
        json!([
            { "name": "work", "issuer": ISSUER, "client_id": CLIENT },
            { "name": "home", "issuer": "https://accounts.example", "client_id": "upland-home" },
        ])
    );
}

#[tokio::test]
async fn an_unconfigured_server_lists_no_providers() {
    let server = server_with(vec![]).await;
    let response = server.get("/api/v1/auth/providers").await;
    response.assert_status_ok();
    assert_eq!(response.json::<Value>(), json!([]));
}

#[tokio::test]
async fn an_unknown_provider_is_named_and_rejected() {
    let server = server_with(vec![provider()]).await;
    let response = oidc_login(&server, "other", "anything").await;
    response.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(
        response.json::<Value>(),
        json!({ "error": { "code": "provider_unknown", "field": "provider" } })
    );
}

#[tokio::test]
async fn a_token_not_signed_by_the_issuers_published_key_gets_nothing() {
    // The crate's signature check is the one saying no here.
    let server = server_with(vec![provider()]).await;
    let forged = signed_token(
        FOREIGN_KEY_PEM,
        "attacker",
        ISSUER,
        CLIENT,
        600,
        EmailClaim::Value("someone@example.com"),
    );
    let response = oidc_login(&server, "work", &forged).await;
    response.assert_status_unauthorized();
    assert_eq!(
        response.json::<Value>(),
        json!({ "error": { "code": "oidc_rejected" } })
    );
}

#[tokio::test]
async fn wrong_issuer_audience_and_expiry_are_all_rejected_by_the_crate() {
    let server = server_with(vec![provider()]).await;
    let cases = [
        (
            "wrong issuer",
            signed_token(
                TEST_KEY_PEM,
                "s",
                "https://evil.invalid",
                CLIENT,
                600,
                EmailClaim::Value("e@x.co"),
            ),
        ),
        (
            "wrong audience",
            signed_token(
                TEST_KEY_PEM,
                "s",
                ISSUER,
                "some-other-client",
                600,
                EmailClaim::Value("e@x.co"),
            ),
        ),
        (
            "expired",
            signed_token(
                TEST_KEY_PEM,
                "s",
                ISSUER,
                CLIENT,
                -60,
                EmailClaim::Value("e@x.co"),
            ),
        ),
    ];
    for (label, token) in cases {
        let response = oidc_login(&server, "work", &token).await;
        response.assert_status_unauthorized();
        assert_eq!(
            response.json::<Value>(),
            json!({ "error": { "code": "oidc_rejected" } }),
            "{label}"
        );
    }
}

#[tokio::test]
async fn a_novel_email_auto_creates_an_account_with_a_derived_username() {
    let server = server_with(vec![provider()]).await;
    let token = signed_token(
        TEST_KEY_PEM,
        "ada-123",
        ISSUER,
        CLIENT,
        600,
        EmailClaim::Value("Ada.Lovelace@Example.com"),
    );
    let response = oidc_login(&server, "work", &token).await;
    response.assert_status_ok();

    let me = me(&server, &token_of(&response)).await;
    assert_eq!(me["username"], "ada.lovelace");
    assert_eq!(me["email"], "ada.lovelace@example.com");
    assert_eq!(me["identities"], json!(["work"]));
}

#[tokio::test]
async fn the_same_identity_signs_into_the_same_account_twice() {
    let server = server_with(vec![provider()]).await;
    let token = signed_token(
        TEST_KEY_PEM,
        "ada-123",
        ISSUER,
        CLIENT,
        600,
        EmailClaim::Value("ada@example.com"),
    );
    let first = oidc_login(&server, "work", &token).await;
    let second = oidc_login(&server, "work", &token).await;
    assert_eq!(
        me(&server, &token_of(&first)).await,
        me(&server, &token_of(&second)).await
    );
}

#[tokio::test]
async fn a_matching_email_links_the_identity_to_the_password_account() {
    let server = server_with(vec![provider()]).await;
    let signup = signup(&server, "grace", "grace@example.com").await;
    let password_token = token_of(&signup);

    let token = signed_token(
        TEST_KEY_PEM,
        "g-1",
        ISSUER,
        CLIENT,
        600,
        EmailClaim::Value("Grace@Example.com"),
    );
    let response = oidc_login(&server, "work", &token).await;
    response.assert_status_ok();

    let by_oidc = me(&server, &token_of(&response)).await;
    let by_password = me(&server, &password_token).await;
    assert_eq!(by_oidc["username"], by_password["username"]);
    assert_eq!(by_oidc["identities"], json!(["work"]));
}

#[tokio::test]
async fn an_unverified_email_assertion_still_links() {
    // ADR-0004 links regardless of email_verified; this test exists to
    // prove the claim is read, ignored, and the link holds anyway.
    let server = server_with(vec![provider()]).await;
    let signup = signup(&server, "grace", "grace@example.com").await;
    let password_token = token_of(&signup);

    let token = signed_token(
        TEST_KEY_PEM,
        "g-2",
        ISSUER,
        CLIENT,
        600,
        EmailClaim::Unverified("grace@example.com"),
    );
    let response = oidc_login(&server, "work", &token).await;
    response.assert_status_ok();
    let by_oidc = me(&server, &token_of(&response)).await;
    assert_eq!(by_oidc["username"], "grace");
    let password_me = me(&server, &password_token).await;
    assert_eq!(by_oidc["email"], password_me["email"]);
}

#[tokio::test]
async fn a_provider_that_asserts_no_email_buys_no_account() {
    let server = server_with(vec![provider()]).await;
    let token = signed_token(
        TEST_KEY_PEM,
        "nameless",
        ISSUER,
        CLIENT,
        600,
        EmailClaim::Absent,
    );
    let response = oidc_login(&server, "work", &token).await;
    response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response.json::<Value>(),
        json!({ "error": { "code": "provider_no_email" } })
    );
}

#[tokio::test]
async fn a_derived_username_collision_gets_a_suffix() {
    let server = server_with(vec![provider()]).await;
    let first = signed_token(
        TEST_KEY_PEM,
        "g-a",
        ISSUER,
        CLIENT,
        600,
        EmailClaim::Value("grace@example.com"),
    );
    let second = signed_token(
        TEST_KEY_PEM,
        "g-b",
        ISSUER,
        CLIENT,
        600,
        EmailClaim::Value("grace@other.example"),
    );
    oidc_login(&server, "work", &first).await.assert_status_ok();
    let response = oidc_login(&server, "work", &second).await;
    response.assert_status_ok();
    assert_eq!(
        me(&server, &token_of(&response)).await["username"],
        "grace-2"
    );
}

#[tokio::test]
async fn a_short_email_local_part_still_yields_a_valid_username() {
    // The username rules (3-32) bind auto-created accounts too.
    let server = server_with(vec![provider()]).await;
    let token = signed_token(
        TEST_KEY_PEM,
        "j-1",
        ISSUER,
        CLIENT,
        600,
        EmailClaim::Value("jo@x.co"),
    );
    let response = oidc_login(&server, "work", &token).await;
    response.assert_status_ok();
    assert_eq!(me(&server, &token_of(&response)).await["username"], "oidc");
}

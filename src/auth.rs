//! **Access token**s (ADR-0004): stateless HS256 JWTs, minimal claims,
//! and the **signing secret** that mints and verifies them.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Every token's `iss` and `aud`: an upland token is for upland and
/// comes from upland. The audience check is what keeps symmetric-key
/// tokens from crossing audit boundaries.
pub const TOKEN_AUDIENCE: &str = "upland";

#[derive(Clone)]
pub struct SigningSecret(Vec<u8>);

impl SigningSecret {
    pub fn random() -> Self {
        use rand::rand_core::Rng as _;
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        Self(bytes.to_vec())
    }

    pub fn hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub fn from_hex(value: &str) -> Result<Self, String> {
        if !value.len().is_multiple_of(2) || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("not a signing secret: expected lowercase hex".to_owned());
        }
        let bytes = (0..value.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&value[i..i + 2], 16).map_err(|e| e.to_string()))
            .collect::<Result<Vec<u8>, String>>()?;
        if !(32..=128).contains(&bytes.len()) {
            return Err(format!(
                "not a signing secret: {} bytes, expected 32 to 128",
                bytes.len()
            ));
        }
        Ok(Self(bytes))
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SigningSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SigningSecret")
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Claims {
    pub sub: String,
    pub iss: String,
    pub aud: String,
    pub iat: i64,
    pub exp: i64,
}

#[derive(Debug, Clone)]
pub struct AccessTokenIssuer {
    secret: SigningSecret,
    ttl_seconds: i64,
}

#[derive(Debug)]
pub enum VerifyError {
    Format(String),
    Expired,
    Issuer,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::Format(reason) => write!(formatter, "invalid access token: {reason}"),
            VerifyError::Expired => write!(formatter, "invalid access token: expired"),
            VerifyError::Issuer => write!(formatter, "invalid access token: issuer"),
        }
    }
}

impl AccessTokenIssuer {
    pub fn new(secret: SigningSecret, ttl_seconds: u64) -> Self {
        Self {
            secret,
            ttl_seconds: ttl_seconds.min(i64::MAX as u64) as i64,
        }
    }

    pub fn mint(&self, subject: &str) -> Result<String, String> {
        self.mint_at(subject, now())
    }

    pub fn mint_at(&self, subject: &str, now: i64) -> Result<String, String> {
        let claims = Claims {
            sub: subject.to_owned(),
            iss: TOKEN_AUDIENCE.to_owned(),
            aud: TOKEN_AUDIENCE.to_owned(),
            iat: now,
            exp: now.saturating_add(self.ttl_seconds),
        };
        let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
        let key = jsonwebtoken::EncodingKey::from_secret(self.secret.expose())
            .map_err(|error| error.to_string())?;
        jsonwebtoken::encode(&header, &claims, &key).map_err(|error| error.to_string())
    }

    pub fn verify(&self, token: &str) -> Result<Claims, VerifyError> {
        self.verify_at(token, now())
    }

    pub fn verify_at(&self, token: &str, now: i64) -> Result<Claims, VerifyError> {
        let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
        validation.set_audience(&[TOKEN_AUDIENCE]);
        // Expiry is judged against the injected clock below, not the crate's.
        validation.validate_exp = false;
        let key = jsonwebtoken::DecodingKey::from_secret(self.secret.expose())
            .map_err(|error| VerifyError::Format(error.to_string()))?;
        let claims = jsonwebtoken::decode::<Claims>(token, &key, &validation)
            .map_err(|error| VerifyError::Format(error.to_string()))?
            .claims;
        if claims.iss != TOKEN_AUDIENCE {
            return Err(VerifyError::Issuer);
        }
        if claims.exp <= now {
            return Err(VerifyError::Expired);
        }
        Ok(claims)
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after 1970")
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret() -> SigningSecret {
        SigningSecret::from_hex(&"ab".repeat(32)).expect("test secret")
    }

    #[test]
    fn a_random_secret_is_32_bytes_of_lowercase_hex() {
        let hex = SigningSecret::random().hex();
        assert_eq!(hex.len(), 64);
        assert!(
            hex.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        assert_ne!(hex, SigningSecret::random().hex());
    }

    #[test]
    fn a_minted_token_round_trips_to_its_claims() {
        let issuer = AccessTokenIssuer::new(secret(), 3600);
        let token = issuer.mint_at("42", 1_000_000).expect("mint");
        let claims = issuer.verify_at(&token, 1_000_001).expect("verify");
        assert_eq!(claims.sub, "42");
        assert_eq!(claims.iss, TOKEN_AUDIENCE);
        assert_eq!(claims.aud, TOKEN_AUDIENCE);
        assert_eq!(claims.iat, 1_000_000);
        assert_eq!(claims.exp, 1_003_600);
    }

    #[test]
    fn a_token_expires() {
        let issuer = AccessTokenIssuer::new(secret(), 60);
        let token = issuer.mint_at("1", 500).expect("mint");
        issuer.verify_at(&token, 559).expect("still valid");
        let error = issuer.verify_at(&token, 561).expect_err("expired");
        assert!(matches!(error, VerifyError::Expired));
    }

    #[test]
    fn another_secret_cannot_verify() {
        let issuer = AccessTokenIssuer::new(secret(), 60);
        let token = issuer.mint_at("1", 500).expect("mint");
        let other = AccessTokenIssuer::new(SigningSecret::from_hex(&"cd".repeat(32)).unwrap(), 60);
        other.verify_at(&token, 501).expect_err("foreign signature");
    }

    #[test]
    fn a_tampered_payload_is_rejected() {
        let issuer = AccessTokenIssuer::new(secret(), 60);
        let token = issuer.mint_at("1", 500).expect("mint");
        let parts: Vec<&str> = token.split('.').collect();
        let tampered = format!("{}.{}.", parts[0], parts[1]);
        issuer.verify_at(&tampered, 501).expect_err("tampered");
    }

    #[test]
    fn the_wrong_audience_is_rejected() {
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
            &Claims {
                sub: "1".to_owned(),
                iss: TOKEN_AUDIENCE.to_owned(),
                aud: "somewhere-else".to_owned(),
                iat: 500,
                exp: 560,
            },
            &jsonwebtoken::EncodingKey::from_secret(secret().expose()).unwrap(),
        )
        .expect("encode");
        let issuer = AccessTokenIssuer::new(secret(), 60);
        issuer.verify_at(&token, 501).expect_err("wrong audience");
    }

    #[test]
    fn the_wrong_issuer_is_rejected() {
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
            &Claims {
                sub: "1".to_owned(),
                iss: "not-upland".to_owned(),
                aud: TOKEN_AUDIENCE.to_owned(),
                iat: 500,
                exp: 560,
            },
            &jsonwebtoken::EncodingKey::from_secret(secret().expose()).unwrap(),
        )
        .expect("encode");
        let issuer = AccessTokenIssuer::new(secret(), 60);
        let error = issuer.verify_at(&token, 501).expect_err("wrong issuer");
        assert!(matches!(error, VerifyError::Issuer));
    }

    #[test]
    fn a_secret_never_appears_in_debug_output() {
        let text = format!("{:?}", secret());
        assert!(text.contains("SigningSecret"));
        assert!(!text.contains("abab"));
    }
}

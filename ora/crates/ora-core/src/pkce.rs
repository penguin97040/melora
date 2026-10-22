//! Proof Key for Code Exchange (RFC 7636, S256) and the OAuth `state` value.

use crate::secret::Secret;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};

pub struct Pkce {
    pub verifier: Secret,
    pub challenge: String,
}

/// 64 random bytes give an 86-character verifier (RFC 7636 allows 43 to 128).
pub fn generate() -> Result<Pkce, getrandom::Error> {
    let mut bytes = [0u8; 64];
    getrandom::fill(&mut bytes)?;
    let verifier = Secret::new(URL_SAFE_NO_PAD.encode(bytes));
    bytes.fill(0);
    let challenge = challenge(verifier.expose());
    Ok(Pkce {
        verifier,
        challenge,
    })
}

pub fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// 32 random bytes, URL-safe, for matching the callback to this sign-in.
pub fn state() -> Result<Secret, getrandom::Error> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)?;
    let value = Secret::new(URL_SAFE_NO_PAD.encode(bytes));
    bytes.fill(0);
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn matches_rfc_7636_appendix_b() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn verifiers_are_long_url_safe_and_random() {
        let mut seen = HashSet::new();
        for _ in 0..32 {
            let pkce = generate().unwrap();
            let verifier = pkce.verifier.expose().to_owned();
            assert_eq!(verifier.len(), 86);
            assert!(verifier
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
            assert_eq!(pkce.challenge, challenge(&verifier));
            assert!(seen.insert(verifier));
        }
    }

    #[test]
    fn states_are_long_and_unique() {
        let mut seen = HashSet::new();
        for _ in 0..32 {
            let value = state().unwrap().expose().to_owned();
            assert_eq!(value.len(), 43);
            assert!(seen.insert(value));
        }
    }
}

//! Proof Key for Code Exchange (RFC 7636, S256) and the OAuth `state` value.

use crate::secret::Secret;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub struct Pkce {
    pub verifier: Secret,
    pub challenge: String,
}

/// 64 random bytes give an 86-character verifier (RFC 7636 allows 43 to 128).
pub fn generate() -> Result<Pkce, getrandom::Error> {
    let verifier = random::<64>()?;
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
    random::<32>()
}

/// `N` random bytes, URL-safe. The raw bytes are wiped however this returns, which a plain
/// `fill(0)` doesn't promise: the optimiser may drop a store that is never read.
fn random<const N: usize>() -> Result<Secret, getrandom::Error> {
    let mut bytes = Zeroizing::new([0u8; N]);
    getrandom::fill(bytes.as_mut_slice())?;
    Ok(Secret::new(URL_SAFE_NO_PAD.encode(bytes.as_slice())))
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

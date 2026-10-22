//! A wrapper for tokens, codes and verifiers. It never prints its value, can't be serialised,
//! and is wiped from memory when dropped.

use std::fmt;
use zeroize::Zeroize;

#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The raw value, for the one place that must send it (an HTTP body or header).
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Compares without returning early on the first differing byte.
    pub fn matches(&self, candidate: &str) -> bool {
        let (a, b) = (self.0.as_bytes(), candidate.as_bytes());
        if a.len() != b.len() {
            return false;
        }
        a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_hides_the_value() {
        let value = Secret::new("synthetic-value");
        assert_eq!(format!("{value:?}"), "[redacted]");
        assert_eq!(format!("{:?}", Some(value)), "Some([redacted])");
    }

    #[test]
    fn matching_checks_every_byte_and_length() {
        let value = Secret::new("abc");
        assert!(value.matches("abc"));
        assert!(!value.matches("abd"));
        assert!(!value.matches("ab"));
        assert!(!value.matches("abcd"));
    }
}

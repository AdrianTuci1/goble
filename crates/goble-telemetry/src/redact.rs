//! Scrubbing before anything is written or sent.
//!
//! A crash report is built from a fixed set of fields, but a panic message or a
//! log line is free text and can contain anything a program ever printed. So
//! every string that goes into a report passes through here: known secret
//! values, credential-shaped literals, email addresses, and the user's home
//! directory path.
//!
//! This is a safety net, not a licence: the rule in
//! `.agents/07-observability/logs.md` still holds — do not log secrets in the
//! first place.

use std::fmt;
use std::sync::OnceLock;

use regex::Regex;

/// The placeholder that replaces anything withheld.
pub const REDACTED: &str = "[redacted]";

struct Patterns {
    /// `authorization: bearer <token>` — handled before anything else, because
    /// the generic assignment rule would stop at the word `bearer` and leave the
    /// token in place.
    credential_header: Regex,
    /// `authorization: Bearer xyz`, `token=...`, `password: ...`
    assignment: Regex,
    /// Credential literals with a recognisable prefix.
    prefixed: Regex,
    /// Long unbroken runs of key-shaped characters: base64, hex, JWTs.
    opaque: Regex,
    email: Regex,
}

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| Patterns {
        credential_header: Regex::new(
            r"(?i)\b(authorization|proxy-authorization)\b\s*[:=]?\s*\bbearer\b\s+[A-Za-z0-9._~+/=-]{4,}",
        )
        .expect("static pattern"),
        assignment: Regex::new(
            r"(?i)\b(authorization|bearer|token|api[_-]?key|access[_-]?key|secret|password|passwd|credential)\b\s*[:=]?\s*[A-Za-z0-9._~+/=-]{6,}",
        )
        .expect("static pattern"),
        prefixed: Regex::new(
            r"\b(?:sk|xai|gsk|ghp|gho|ghs|ghu|github_pat|glpat|npm|pypi|xoxb|xoxp|xoxa|xoxr)-[A-Za-z0-9_-]{8,}|\b(?:AKIA|ASIA)[A-Z0-9]{12,}",
        )
        .expect("static pattern"),
        opaque: Regex::new(r"[A-Za-z0-9+/]{32,}={0,2}").expect("static pattern"),
        email: Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b")
            .expect("static pattern"),
    })
}

/// Replaces anything that must not leave the machine.
pub struct Redactor {
    home: Option<String>,
    secrets: Vec<String>,
}

impl fmt::Debug for Redactor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never print the secrets themselves.
        f.debug_struct("Redactor")
            .field("home", &self.home)
            .field("secrets", &self.secrets.len())
            .finish()
    }
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new()
    }
}

impl Redactor {
    /// A redactor that knows the user's home directory.
    pub fn new() -> Self {
        Self { home: home_directory(), secrets: Vec::new() }
    }

    pub fn with_home(mut self, home: impl Into<String>) -> Self {
        let home = home.into();
        self.home = if home.is_empty() { None } else { Some(home) };
        self
    }

    /// Also scrub this exact value wherever it appears. Used for credentials the
    /// app is holding.
    pub fn with_secret(mut self, secret: impl Into<String>) -> Self {
        let secret = secret.into();
        // A short secret would match everything, so it is not usable as a filter.
        if secret.len() >= 4 {
            self.secrets.push(secret);
        }
        self
    }

    pub fn scrub(&self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }

        let mut out = text.to_owned();

        for secret in &self.secrets {
            if out.contains(secret.as_str()) {
                out = out.replace(secret.as_str(), REDACTED);
            }
        }

        if let Some(home) = &self.home {
            out = out.replace(home.as_str(), "~");
            // Windows reports the same directory with backslashes.
            let escaped = home.replace('/', "\\");
            if escaped != *home {
                out = out.replace(escaped.as_str(), "~");
            }
        }

        let patterns = patterns();
        let out = patterns.credential_header.replace_all(&out, |caps: &regex::Captures<'_>| {
            format!("{} {REDACTED}", &caps[1])
        });
        let out = patterns.assignment.replace_all(&out, |caps: &regex::Captures<'_>| {
            format!("{} {REDACTED}", &caps[1])
        });
        let out = patterns.prefixed.replace_all(&out, REDACTED);
        let out = patterns.opaque.replace_all(&out, REDACTED);
        patterns.email.replace_all(&out, "[email]").into_owned()
    }

    pub fn scrub_opt(&self, text: Option<&str>) -> Option<String> {
        text.map(|text| self.scrub(text))
    }
}

fn home_directory() -> Option<String> {
    for name in ["HOME", "USERPROFILE"] {
        if let Ok(value) = std::env::var(name) {
            let value = value.trim().trim_end_matches(['/', '\\']).to_owned();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redactor() -> Redactor {
        Redactor::new().with_home("/Users/ana").with_secret("hunter2-correct-horse")
    }

    #[test]
    fn an_ordinary_panic_message_is_left_alone() {
        let message = "index out of bounds: the len is 3 but the index is 5";
        assert_eq!(redactor().scrub(message), message);
    }

    #[test]
    fn a_bearer_header_loses_its_token() {
        let scrubbed = redactor().scrub("Authorization: Bearer abcdef1234567890");
        assert_eq!(scrubbed, "Authorization [redacted]");
    }

    #[test]
    fn key_shaped_literals_are_removed() {
        let scrubbed = redactor().scrub("failed with sk-proj-abcdefghijklmnop and done");
        assert_eq!(scrubbed, "failed with [redacted] and done");

        let hex = redactor().scrub("commit 8f14e45fceea167a5a36dedd4bea2543f00d1234 failed");
        assert_eq!(hex, "commit [redacted] failed");
    }

    #[test]
    fn emails_are_removed() {
        assert_eq!(
            redactor().scrub("user ana@example.com tried it"),
            "user [email] tried it"
        );
    }

    #[test]
    fn the_home_directory_becomes_a_tilde() {
        let scrubbed = redactor().scrub("cannot open /Users/ana/Projects/goble/config.toml");
        assert_eq!(scrubbed, "cannot open ~/Projects/goble/config.toml");
    }

    #[test]
    fn a_known_secret_is_removed_wherever_it_appears() {
        let scrubbed = redactor().scrub("vault holds hunter2-correct-horse, apparently");
        assert_eq!(scrubbed, "vault holds [redacted], apparently");
    }

    #[test]
    fn short_secrets_are_not_used_as_filters() {
        let redactor = Redactor::new().with_home("/Users/ana").with_secret("abc");
        assert_eq!(redactor.scrub("abc def abc"), "abc def abc");
    }

    #[test]
    fn a_jwt_is_opaque() {
        let scrubbed = redactor()
            .scrub("token eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        assert!(!scrubbed.contains("eyJhbGciOiJIUzI1NiJ9"), "{scrubbed}");
    }

    #[test]
    fn an_empty_string_stays_empty() {
        assert_eq!(redactor().scrub(""), "");
        assert_eq!(redactor().scrub_opt(None), None);
    }
}

//! Sensitive content detection.
//!
//! Runs before any transform stage. If any pattern matches, the input
//! is returned unchanged with `ctx.skipped_sensitive = true`.
//! See §5.5 of the technical spec.

use regex::Regex;

/// Pre-compiled set of secret-detection patterns.
pub struct SecretFilter {
    patterns: Vec<Regex>,
}

impl SecretFilter {
    /// Build the default secret filter with all built-in patterns.
    pub fn new() -> Self {
        let pattern_strings = [
            // PEM private key header
            r"-----BEGIN (RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----",
            // X.509 certificate
            r"-----BEGIN CERTIFICATE-----",
            // Long base64 (likely key/token) — single line, >40 chars
            r"(?m)^[A-Za-z0-9+/]{40,}={0,2}$",
            // API key prefixes (OpenAI, Stripe, etc.)
            r"(sk|pk|rk|ak)[-_][a-zA-Z0-9]{20,}",
            // GitHub personal access token
            r"ghp_[a-zA-Z0-9]{36}",
            // Slack tokens
            r"xox[bpsar]-[a-zA-Z0-9\-]{10,}",
            // JWT (starts with base64 `{"`)
            r"eyJ[a-zA-Z0-9_\-]{10,}\.[a-zA-Z0-9_\-]{10,}",
            // AWS access key ID
            r"AKIA[A-Z0-9]{16}",
        ];

        let patterns = pattern_strings
            .iter()
            .map(|p| Regex::new(p).expect("built-in secret pattern must compile"))
            .collect();

        SecretFilter { patterns }
    }

    /// Returns `true` if the input matches any secret pattern.
    pub fn check(&self, input: &str) -> bool {
        self.patterns.iter().any(|re| re.is_match(input))
    }
}

impl Default for SecretFilter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pem_key_detected() {
        let filter = SecretFilter::new();
        let input = "-----BEGIN RSA PRIVATE KEY-----\nMIIE...\n-----END RSA PRIVATE KEY-----";
        assert!(filter.check(input));
    }

    #[test]
    fn test_jwt_detected() {
        let filter = SecretFilter::new();
        // nosemgrep: generic.secrets.security.detected-jwt-token.detected-jwt-token
        let input = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        assert!(filter.check(input));
    }

    #[test]
    fn test_github_token_detected() {
        let filter = SecretFilter::new();
        // ghp_ + exactly 36 alphanumeric chars
        // nosemgrep: generic.secrets.security.detected-github-token.detected-github-token
        let input = "token: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij";
        assert!(filter.check(input));
    }

    #[test]
    fn test_aws_key_detected() {
        let filter = SecretFilter::new();
        let input = "aws_access_key_id = AKIAIOSFODNN7EXAMPLE";
        assert!(filter.check(input));
    }

    #[test]
    fn test_normal_text_passes() {
        let filter = SecretFilter::new();
        let input = "Hello world. This is a normal paragraph with no secrets.";
        assert!(!filter.check(input));
    }

    #[test]
    fn test_slack_token_detected() {
        let filter = SecretFilter::new();
        let input = "SLACK_TOKEN=xoxb-1234567890-abcdefghijklmnop";
        assert!(filter.check(input));
    }

    #[test]
    fn test_api_key_prefix_detected() {
        let filter = SecretFilter::new();
        // sk- followed by 20+ alphanumeric chars (no hyphens in the run)
        let input = "OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwx";
        assert!(filter.check(input));
    }
}

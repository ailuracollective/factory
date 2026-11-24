use sha2::{Digest, Sha256};

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[allow(dead_code)]
pub fn verify_signature(secret: &str, body: &[u8], header: &str) -> bool {
    if secret.is_empty() {
        return false;
    }
    if !header.starts_with("sha256=") {
        return false;
    }
    use hmac::{Hmac, Mac};
    type HmacSha256 = Hmac<Sha256>;
    let expected_hex = &header[7..];
    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(body);
    let result = mac.finalize().into_bytes();
    let expected = match hex::decode(expected_hex) {
        Ok(v) => v,
        Err(_) => return false,
    };
    constant_time_eq(&expected, &result)
}

#[derive(Debug, PartialEq, Eq)]
pub enum AuthDecision {
    Allowed,
    MissingConfig,
    Denied,
}

pub fn check_secret(configured: &str, provided: &str) -> AuthDecision {
    if configured.is_empty() {
        return AuthDecision::MissingConfig;
    }
    if provided.is_empty() {
        return AuthDecision::Denied;
    }
    if constant_time_eq(configured.as_bytes(), provided.as_bytes()) {
        AuthDecision::Allowed
    } else {
        AuthDecision::Denied
    }
}

pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub fn format_runner_token(bytes: &[u8]) -> String {
    format!("frt_{}", hex::encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_signature_empty_secret_rejects() {
        assert!(!verify_signature("", b"hello", ""));
        assert!(!verify_signature("", b"hello", "sha256=00"));
    }

    #[test]
    fn verify_signature_rejects_bad() {
        assert!(!verify_signature("secret", b"hello", "sha256=deadbeef"));
        assert!(!verify_signature("secret", b"hello", ""));
        assert!(!verify_signature("secret", b"hello", "sha1=deadbeef"));
    }

    #[test]
    fn verify_signature_accepts_valid() {
        use hmac::{Hmac, Mac};
        type H = Hmac<Sha256>;
        let mut mac = H::new_from_slice(b"secret").unwrap();
        mac.update(b"hello");
        let hexsig = hex::encode(mac.finalize().into_bytes());
        let header = format!("sha256={}", hexsig);
        assert!(verify_signature("secret", b"hello", &header));
    }

    #[test]
    fn secret_checks_fail_closed() {
        assert_eq!(check_secret("", ""), AuthDecision::MissingConfig);
        assert_eq!(check_secret("", "anything"), AuthDecision::MissingConfig);
        assert_eq!(check_secret("admin", ""), AuthDecision::Denied);
        assert_eq!(check_secret("admin", "wrong"), AuthDecision::Denied);
        assert_eq!(check_secret("admin", "admin"), AuthDecision::Allowed);
    }

    #[test]
    fn runner_tokens_are_hashed_and_never_time_derived() {
        let token = format_runner_token(&[0xab; 32]);
        assert!(token.starts_with("frt_"));
        assert_eq!(token.len(), 4 + 64);
        assert_eq!(hash_token("token").len(), 64);
        assert_ne!(hash_token("token"), hash_token("token2"));

        let a = format_runner_token(&[1; 32]);
        let b = format_runner_token(&[1; 32]);
        assert_eq!(a, b);
        assert_ne!(a, format_runner_token(&[2; 32]));
    }
}

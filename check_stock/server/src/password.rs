//! Password hashing (Argon2id) and the password policy.
//!
//! Hashes are PHC strings (`$argon2id$v=19$m=19456,t=2,p=1$…`) with a random
//! salt each; the parameters are the OWASP recommendation and part of the
//! string, so they can be raised later without breaking existing hashes.
//! Hashing is deliberately slow (tens of milliseconds): run it on a blocking
//! thread, never on the async workers.

use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;

/// Shortest allowed password, in characters.
pub const MIN_PASSWORD_CHARS: usize = 12;
/// Longest allowed password, in bytes (keeps hashing cost bounded).
pub const MAX_PASSWORD_BYTES: usize = 1024;

#[derive(Debug)]
pub struct PasswordError(String);

impl std::fmt::Display for PasswordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "password hashing failed: {}", self.0)
    }
}

impl std::error::Error for PasswordError {}

/// Checks a new password. Length is what counts (NIST SP 800-63B): no
/// composition rules, but at least [`MIN_PASSWORD_CHARS`] characters and not
/// the user name. Returns a message for the user.
pub fn check_policy(password: &str, username: &str) -> Result<(), String> {
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(format!(
            "The password may have at most {MAX_PASSWORD_BYTES} bytes."
        ));
    }
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(format!(
            "The password needs at least {MIN_PASSWORD_CHARS} characters."
        ));
    }
    if password.trim().is_empty() {
        return Err("The password must not be only spaces.".to_string());
    }
    if password.trim().eq_ignore_ascii_case(username.trim()) {
        return Err("The password must not be the user name.".to_string());
    }
    Ok(())
}

/// Argon2id hash of `password` with a fresh random salt.
pub fn hash(password: &str) -> Result<String, PasswordError> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| PasswordError(e.to_string()))
}

/// Whether `password` matches the stored hash. A malformed hash never
/// matches. The comparison inside is constant-time.
pub fn verify(password: &str, stored_hash: &str) -> bool {
    if password.len() > MAX_PASSWORD_BYTES {
        return false;
    }
    match PasswordHash::new(stored_hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Hash of a random password nobody knows. Logins for unknown user names are
/// checked against it, so they take as long as real ones and the response
/// time does not reveal which names exist.
pub fn dummy_hash() -> Result<String, PasswordError> {
    let mut random = [0u8; 32];
    getrandom::fill(&mut random).map_err(|e| PasswordError(e.to_string()))?;
    let secret: String = random.iter().map(|b| format!("{b:02x}")).collect();
    hash(&secret)
}

#[cfg(test)]
#[path = "password_tests.rs"]
mod tests;

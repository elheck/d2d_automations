use super::*;

#[test]
fn hashes_are_argon2id_with_the_recommended_parameters() {
    let h = hash("correct horse battery").unwrap();
    assert!(h.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"), "{h}");
}

#[test]
fn verify_accepts_only_the_right_password() {
    let h = hash("correct horse battery").unwrap();
    assert!(verify("correct horse battery", &h));
    assert!(!verify("correct horse batterY", &h));
    assert!(!verify("", &h));
}

#[test]
fn every_hash_has_its_own_salt() {
    assert_ne!(
        hash("same password twice").unwrap(),
        hash("same password twice").unwrap()
    );
}

#[test]
fn malformed_or_foreign_hashes_never_match() {
    assert!(!verify("anything at all", ""));
    assert!(!verify("anything at all", "not a hash"));
    assert!(!verify("anything at all", "$2b$12$abcdefghijklmnopqrstuv"));
}

#[test]
fn overlong_passwords_are_refused_without_hashing() {
    let h = hash("correct horse battery").unwrap();
    assert!(!verify(&"x".repeat(MAX_PASSWORD_BYTES + 1), &h));
}

#[test]
fn the_dummy_hash_matches_nothing_guessable() {
    let dummy = dummy_hash().unwrap();
    assert!(dummy.starts_with("$argon2id$"));
    for guess in ["", "password", "admin", "123456789012"] {
        assert!(!verify(guess, &dummy));
    }
}

#[test]
fn policy() {
    assert!(check_policy("twelve chars", "anna").is_ok());
    assert!(check_policy("eleven char", "anna")
        .unwrap_err()
        .contains("12"));
    assert!(
        check_policy("äöüäöüäöüäöü", "anna").is_ok(),
        "counts characters, not bytes"
    );
    assert!(check_policy(&"x".repeat(MAX_PASSWORD_BYTES + 1), "anna").is_err());
    assert!(check_policy(&" ".repeat(20), "anna").is_err());
    assert!(check_policy("verkaufsleitung", "Verkaufsleitung").is_err());
    assert!(check_policy("  verkaufsleitung  ", "verkaufsleitung").is_err());
}

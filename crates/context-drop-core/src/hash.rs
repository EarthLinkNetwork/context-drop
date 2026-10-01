//! Content hashing (SHA-256), used for item integrity and consecutive-dedupe.

use sha2::{Digest, Sha256};

/// Lowercase hex SHA-256 of the given bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let out = hasher.finalize();
    let mut s = String::with_capacity(out.len() * 2);
    for b in out {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn unicode_is_hashed_by_bytes() {
        // Japanese text hashes deterministically by its UTF-8 bytes.
        let h1 = sha256_hex("こんにちは".as_bytes());
        let h2 = sha256_hex("こんにちは".as_bytes());
        assert_eq!(h1, h2);
        assert_ne!(h1, sha256_hex("こんばんは".as_bytes()));
    }
}

//! Collision-resistant identifiers.
//!
//! We use UUIDv7: time-ordered (so packet directories sort chronologically)
//! yet backed by random bits, so ids are collision-resistant. Per spec, a
//! timestamp alone is never used as an id.

use uuid::Uuid;

/// A new packet id, e.g. `01936f7e-...`.
pub fn new_packet_id() -> String {
    Uuid::now_v7().to_string()
}

/// A new packet-item id.
pub fn new_item_id() -> String {
    Uuid::now_v7().to_string()
}

/// A new claim id.
pub fn new_claim_id() -> String {
    Uuid::now_v7().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique() {
        let mut seen = HashSet::new();
        for _ in 0..10_000 {
            assert!(seen.insert(new_packet_id()), "duplicate id generated");
        }
    }

    #[test]
    fn ids_are_not_pure_timestamps() {
        // Two ids minted back-to-back must differ even within the same millisecond.
        let a = new_packet_id();
        let b = new_packet_id();
        assert_ne!(a, b);
    }
}

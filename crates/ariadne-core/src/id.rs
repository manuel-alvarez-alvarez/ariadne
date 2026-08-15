//! ULID-based identifiers.
//!
//! Ids are stored and transported as their canonical 26-char string form.
//! ULIDs are lexicographically time-sortable, which the API layer relies on
//! for keyset pagination (`?after=<id>`) and audit ordering. Generation is
//! monotonic within the process (the daemon is the sole id creator), so ids
//! created in the same millisecond still sort in creation order.

use std::sync::Mutex;

use ulid::{Generator, Ulid};

static GENERATOR: Mutex<Option<Generator>> = Mutex::new(None);

/// Generate a new id in canonical lowercase ULID form.
pub fn new_id() -> String {
    let mut guard = GENERATOR
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let generator = guard.get_or_insert_with(Generator::new);
    // Overflow within one millisecond is astronomically unlikely; fall back
    // to a plain ulid rather than panicking.
    generator
        .generate()
        .unwrap_or_else(|_| Ulid::generate())
        .to_string()
        .to_lowercase()
}

/// Validate that a string is a well-formed ULID.
pub fn is_valid(id: &str) -> bool {
    Ulid::from_string(&id.to_uppercase()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_are_valid_and_strictly_ordered() {
        let ids: Vec<String> = (0..1000).map(|_| new_id()).collect();
        for pair in ids.windows(2) {
            assert!(is_valid(&pair[0]));
            assert!(
                pair[0] < pair[1],
                "ids must be strictly increasing: {} >= {}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(!is_valid("not-an-id"));
        assert!(!is_valid(""));
    }
}

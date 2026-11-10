//! Approval digests shared by the commands that accept a human review.
//!
//! `fetch --accept-first <plaintext sha>`, `apply --confirm-first <keybox sha>`
//! and `apply --allow-identity-change <keybox sha>` all bind the approval to
//! exactly the version that was shown; a swapped artifact is refused.

use crate::error::{Stage, SyncError};

/// `full` (lowercase hex) matches a confirmation value (full or >= 16-char prefix).
pub fn sha_confirms(full: &str, given: &str) -> bool {
    given.len() >= 16 && full.starts_with(given)
}

/// The reviewed artifact is no longer the staged one.
pub fn confirmation_mismatch(what: &str) -> SyncError {
    SyncError::new(
        Stage::State,
        "confirmation_mismatch",
        format!("the {what} changed since it was reviewed; review it again"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_and_full_digests_match() {
        let full = "abcdef0123456789abcdef";
        assert!(sha_confirms(full, full));
        assert!(sha_confirms(full, "abcdef0123456789"));
        assert!(!sha_confirms(full, "abcdef012345678"), "prefix too short");
        assert!(!sha_confirms(full, "abcdef0123456789ffff"));
        assert!(!sha_confirms(full, "ABCDEF0123456789"));
    }

    #[test]
    fn mismatch_is_a_state_error() {
        let e = confirmation_mismatch("staged keybox");
        assert_eq!(e.code, "confirmation_mismatch");
        assert_eq!(e.stage, Stage::State);
    }
}

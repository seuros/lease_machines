use core::time::Duration;

use crate::FencingToken;

/// An opaque numeric owner identity, scoped and authenticated by the adapter.
///
/// Supplied by the caller, never issued by the authority: it carries no scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OwnerId(u64);

impl OwnerId {
    /// Construct an owner identity. Zero is a valid identity.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Numeric identity for storage or transport adapters.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// One grant in a resource's serialized authority history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lease {
    owner: OwnerId,
    token: FencingToken,
    expires_at: Duration,
}

impl Lease {
    /// Rebuild a committed grant from storage; see
    /// [`Checkpoint::from_parts`](crate::Checkpoint::from_parts).
    #[must_use]
    pub const fn new(owner: OwnerId, token: FencingToken, expires_at: Duration) -> Self {
        Self {
            owner,
            token,
            expires_at,
        }
    }

    /// Owner identity that received this grant.
    #[must_use]
    pub const fn owner(self) -> OwnerId {
        self.owner
    }

    /// Token to present to protected mutations; it carries the authority's scope.
    #[must_use]
    pub const fn token(self) -> FencingToken {
        self.token
    }

    /// Expiration relative to the authority's monotonic origin.
    #[must_use]
    pub const fn expires_at(self) -> Duration {
        self.expires_at
    }
}

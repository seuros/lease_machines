use core::time::Duration;

use crate::{FencingToken, Rejection};

/// An opaque numeric owner identity, scoped and authenticated by the adapter.
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
    pub(crate) owner: OwnerId,
    pub(crate) token: FencingToken,
    pub(crate) expires_at: Duration,
}

impl Lease {
    /// Owner identity that received this grant.
    #[must_use]
    pub const fn owner(self) -> OwnerId {
        self.owner
    }

    /// Resource-scoped token to present to protected mutations.
    #[must_use]
    pub const fn token(self) -> FencingToken {
        self.token
    }

    /// Expiration relative to the authority's monotonic origin.
    #[must_use]
    pub const fn expires_at(self) -> Duration {
        self.expires_at
    }

    pub(crate) fn rejection(
        self,
        owner: OwnerId,
        token: FencingToken,
        now: Duration,
    ) -> Option<Rejection> {
        if self.owner != owner {
            Some(Rejection::WrongOwner)
        } else if self.token != token {
            Some(Rejection::StaleToken)
        } else if now >= self.expires_at {
            Some(Rejection::Expired)
        } else {
            None
        }
    }
}

use core::time::Duration;

use periscope_machines::{Issuer, scoped};

use crate::Lease;

/// Committed authority state: what to persist before exposing a decision.
///
/// It names its scope, and [`LeaseAuthority::restored`](crate::LeaseAuthority::restored)
/// refuses it under any other. It is deliberately not `Copy`: one checkpoint
/// restores one authority. Restoring an older checkpoint than the newest
/// committed one reissues tokens, and nothing in a pure value can detect that;
/// see the persistence contract in the crate documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    /// The token sequence as committed: its scope and how many were issued.
    tokens: Issuer,
    lease: Option<Lease>,
    observed: Duration,
}

impl Checkpoint {
    /// Rebuild committed state from storage columns.
    ///
    /// `issued` is how many tokens the history has issued, which is also the
    /// sequence number of the last one. `None` when `issued` is beyond the
    /// scope's token space, or when `lease` is not the grant that history
    /// ends with: every grant takes the next token, so a live row carries
    /// exactly the last token issued, in this scope. A lease under an older
    /// token was superseded, and one under another scope or an unissued token
    /// was never granted here. Use `Duration::ZERO` as `observed` when storage
    /// keeps no clock watermark.
    #[must_use]
    pub const fn from_parts(
        scope: u16,
        issued: u64,
        lease: Option<Lease>,
        observed: Duration,
    ) -> Option<Self> {
        let Some(tokens) = Issuer::resumed(scope, issued) else {
            return None;
        };
        if let Some(lease) = lease
            && (issued == 0 || lease.token().raw() != scoped(scope, issued))
        {
            return None;
        }
        Some(Self {
            tokens,
            lease,
            observed,
        })
    }

    /// State taken from a live authority, whose invariants already hold.
    pub(crate) const fn committed(
        tokens: Issuer,
        lease: Option<Lease>,
        observed: Duration,
    ) -> Self {
        Self {
            tokens,
            lease,
            observed,
        }
    }

    /// Spend the checkpoint: the token sequence, lease, observed.
    pub(crate) const fn into_parts(self) -> (Issuer, Option<Lease>, Duration) {
        (self.tokens, self.lease, self.observed)
    }

    /// The scope of the authority this state belongs to.
    #[must_use]
    pub const fn scope(&self) -> u16 {
        self.tokens.scope()
    }

    /// How many tokens this history has issued, releases included; the next
    /// token is sequence `issued + 1`.
    #[must_use]
    pub const fn issued(&self) -> u64 {
        self.tokens.issued()
    }

    /// Last grant; it may already be expired at the latest observation.
    #[must_use]
    pub const fn lease(&self) -> Option<Lease> {
        self.lease
    }

    /// Latest honoured clock observation. A refused request never moves it.
    #[must_use]
    pub const fn observed(&self) -> Duration {
        self.observed
    }
}

use core::time::Duration;

use crate::{FencingToken, Lease, OwnerId};

/// Why an authority rejected a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Rejection {
    /// An unexpired lease already occupies the resource.
    Occupied,
    /// No grant exists to renew or release.
    NoLease,
    /// The request's owner does not match the current grant.
    WrongOwner,
    /// The request's token does not match the current grant.
    StaleToken,
    /// The current grant has expired.
    Expired,
    /// Time moved backwards relative to a previous observation.
    ClockRegressed,
    /// TTL is zero or its addition overflows the duration range.
    InvalidTtl,
    /// No greater fencing token can be issued.
    TokenExhausted,
}

/// How an authority treats an observation earlier than its last one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ClockPolicy {
    /// Reject the request with `Rejection::ClockRegressed`, changing nothing.
    #[default]
    Reject,
    /// Evaluate the request at the last observation instead of the earlier `now`.
    Clamp,
}

/// A proposed state-machine outcome, to commit before exposing success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Decision {
    /// A new token and lease were assigned.
    Granted(Lease),
    /// The current lease was renewed without changing its token.
    Renewed(Lease),
    /// The current lease was released.
    Released,
    /// The request was rejected.
    Rejected(Rejection),
}

/// Opaque authority state with no promised wire representation.
///
/// Restore only the newest committed state with the same monotonic clock origin.
/// Copying or rolling back this value does not create an independent authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint {
    last_issued: Option<FencingToken>,
    lease: Option<Lease>,
    observed: Duration,
}

impl Checkpoint {
    /// Rebuild committed state from storage columns.
    ///
    /// Returns `None` when a lease carries a token the history never issued.
    /// Use `Duration::ZERO` as `observed` when storage keeps no clock watermark.
    #[must_use]
    pub const fn from_parts(
        last_issued: Option<FencingToken>,
        lease: Option<Lease>,
        observed: Duration,
    ) -> Option<Self> {
        if let Some(lease) = lease {
            match last_issued {
                Some(highest) if lease.token.value() <= highest.value() => {}
                _ => return None,
            }
        }
        Some(Self {
            last_issued,
            lease,
            observed,
        })
    }

    /// Greatest token issued in this authority history, even after release.
    #[must_use]
    pub const fn last_issued(self) -> Option<FencingToken> {
        self.last_issued
    }

    /// Last grant; it may already be expired at the latest observation.
    #[must_use]
    pub const fn lease(self) -> Option<Lease> {
        self.lease
    }

    /// Latest accepted clock observation, including rejected requests.
    #[must_use]
    pub const fn observed(self) -> Duration {
        self.observed
    }
}

/// A deterministic lease authority for one resource and token namespace.
///
/// Runtime adapters must serialize and durably commit transitions before
/// exposing success. This type provides no distributed agreement or I/O.
#[derive(Debug, PartialEq, Eq)]
pub struct LeaseAuthority {
    state: Checkpoint,
    clock: ClockPolicy,
}

impl Default for LeaseAuthority {
    fn default() -> Self {
        Self::new()
    }
}

impl LeaseAuthority {
    /// Start a **fresh** authority namespace. Never use this to reset an old one.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: Checkpoint {
                last_issued: None,
                lease: None,
                observed: Duration::ZERO,
            },
            clock: ClockPolicy::Reject,
        }
    }

    /// Restore the newest committed checkpoint under the same clock origin.
    ///
    /// The caller must prevent simultaneous serving by the previous authority.
    #[must_use]
    pub const fn restore(checkpoint: Checkpoint) -> Self {
        Self {
            state: checkpoint,
            clock: ClockPolicy::Reject,
        }
    }

    /// Choose how earlier-than-observed clocks are handled; `Reject` by default.
    #[must_use]
    pub const fn with_clock_policy(mut self, clock: ClockPolicy) -> Self {
        self.clock = clock;
        self
    }

    /// Policy applied to regressing observations.
    #[must_use]
    pub const fn clock_policy(&self) -> ClockPolicy {
        self.clock
    }

    /// Capture state for a host-controlled commit or deterministic simulation.
    #[must_use]
    pub const fn checkpoint(&self) -> Checkpoint {
        self.state
    }

    /// Acquire an unoccupied/expired resource with a new, strictly greater token.
    #[must_use]
    pub fn acquire(&mut self, owner: OwnerId, now: Duration, ttl: Duration) -> Decision {
        self.grant(owner, now, ttl, false)
    }

    /// Acquire like `acquire`, or rotate the token of `owner`'s own unexpired grant.
    ///
    /// Another owner's unexpired grant is still `Occupied`. The previous token
    /// becomes stale, so in-flight work under it is fenced.
    #[must_use]
    pub fn reacquire(&mut self, owner: OwnerId, now: Duration, ttl: Duration) -> Decision {
        self.grant(owner, now, ttl, true)
    }

    fn grant(&mut self, owner: OwnerId, now: Duration, ttl: Duration, rotate: bool) -> Decision {
        let now = match self.observe(now) {
            Ok(now) => now,
            Err(reason) => return Decision::Rejected(reason),
        };
        let Some(expires_at) = expiration(now, ttl) else {
            return Decision::Rejected(Rejection::InvalidTtl);
        };
        if self
            .state
            .lease
            .is_some_and(|lease| now < lease.expires_at && !(rotate && lease.owner == owner))
        {
            return Decision::Rejected(Rejection::Occupied);
        }
        let Some(token) = FencingToken::next(self.state.last_issued) else {
            return Decision::Rejected(Rejection::TokenExhausted);
        };
        let lease = Lease {
            owner,
            token,
            expires_at,
        };
        self.state.last_issued = Some(token);
        self.state.lease = Some(lease);
        Decision::Granted(lease)
    }

    /// Renew only the current unexpired grant, retaining its token.
    ///
    /// A shorter requested TTL does not shorten the previous expiration.
    #[must_use]
    pub fn renew(
        &mut self,
        owner: OwnerId,
        token: FencingToken,
        now: Duration,
        ttl: Duration,
    ) -> Decision {
        let now = match self.observe(now) {
            Ok(now) => now,
            Err(reason) => return Decision::Rejected(reason),
        };
        let Some(mut lease) = self.state.lease else {
            return Decision::Rejected(Rejection::NoLease);
        };
        if let Some(reason) = lease.rejection(owner, token, now) {
            return Decision::Rejected(reason);
        }
        let Some(expires_at) = expiration(now, ttl) else {
            return Decision::Rejected(Rejection::InvalidTtl);
        };
        lease.expires_at = lease.expires_at.max(expires_at);
        self.state.lease = Some(lease);
        Decision::Renewed(lease)
    }

    /// Release only the current unexpired grant, retaining the token high-water.
    #[must_use]
    pub fn release(&mut self, owner: OwnerId, token: FencingToken, now: Duration) -> Decision {
        let now = match self.observe(now) {
            Ok(now) => now,
            Err(reason) => return Decision::Rejected(reason),
        };
        let Some(lease) = self.state.lease else {
            return Decision::Rejected(Rejection::NoLease);
        };
        if let Some(reason) = lease.rejection(owner, token, now) {
            return Decision::Rejected(reason);
        }
        self.state.lease = None;
        Decision::Released
    }

    fn observe(&mut self, now: Duration) -> Result<Duration, Rejection> {
        if now < self.state.observed {
            return match self.clock {
                ClockPolicy::Reject => Err(Rejection::ClockRegressed),
                ClockPolicy::Clamp => Ok(self.state.observed),
            };
        }
        self.state.observed = now;
        Ok(now)
    }
}

const fn expiration(now: Duration, ttl: Duration) -> Option<Duration> {
    if ttl.is_zero() {
        None
    } else {
        now.checked_add(ttl)
    }
}

#[cfg(test)]
mod tests;

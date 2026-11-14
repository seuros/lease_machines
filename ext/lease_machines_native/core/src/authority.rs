use core::time::Duration;

use periscope_machines::Issuer;

use crate::{Checkpoint, FencingToken, Lease, OwnerId, Refusal};

/// How an authority treats a `now` earlier than its latest honoured observation.
///
/// Under either policy the watermark is the latest `now` of an *honoured*
/// call: a refused call records nothing, so it is checked against the
/// watermark but never moves it. A grant answered [`Refusal::Expired`] at one
/// instant may therefore still be renewed at an earlier one that is not
/// behind the watermark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClockPolicy {
    /// Refuse the request with [`Refusal::ClockRegressed`].
    #[default]
    Reject,
    /// Evaluate the request at the watermark instead of the earlier `now`:
    /// expiry checks and new deadlines use the later time, so a backwards
    /// step cannot undo what an honoured call did. It cannot bring back a
    /// grant that was released or replaced, and it cannot shorten one. A
    /// clamped call that is honoured leaves the watermark where it was; one
    /// that is refused for another reason changes nothing, like any refusal.
    ///
    /// It does not make a refused instant final: see [`Refusal::Expired`].
    Clamp,
}

/// A deterministic lease authority for one resource and one token scope.
///
/// Runtime adapters must serialize and durably commit transitions before
/// exposing success. This type provides no distributed agreement or I/O.
/// Cloning it does not create a second authority: two instances serving one
/// scope issue the same tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseAuthority {
    /// Issues the fencing tokens; it also holds the authority's scope.
    tokens: Issuer,
    lease: Option<Lease>,
    observed: Duration,
    clock: ClockPolicy,
}

impl Default for LeaseAuthority {
    fn default() -> Self {
        Self::new()
    }
}

impl LeaseAuthority {
    /// Start a **fresh** token namespace in scope zero. Never use this to
    /// reset an old one: it issues the same tokens again.
    #[must_use]
    pub const fn new() -> Self {
        Self::with_scope(0)
    }

    /// Start a **fresh** token namespace whose tokens carry `scope`.
    ///
    /// Sequence numbers count from one, so two authorities in one scope issue
    /// identical tokens and a fence cannot tell them apart. Give every
    /// authority whose tokens could meet its own scope; a token of another
    /// scope is then [`Refusal::Unknown`] to the authority and to a fence of
    /// this scope. Keeping scopes distinct is the caller's job.
    #[must_use]
    pub const fn with_scope(scope: u16) -> Self {
        Self {
            tokens: Issuer::new(scope),
            lease: None,
            observed: Duration::ZERO,
            clock: ClockPolicy::Reject,
        }
    }

    /// Restore the newest committed checkpoint of `scope`, under the same
    /// clock origin, with the clock policy stated again.
    ///
    /// The scope is the caller's, never read out of the checkpoint: `None`
    /// when the checkpoint is of another scope. The policy is configuration,
    /// not committed state, so it is an argument rather than a silent default.
    /// The caller must prevent simultaneous serving by the previous authority.
    #[must_use]
    pub const fn restored(scope: u16, checkpoint: Checkpoint, clock: ClockPolicy) -> Option<Self> {
        let (tokens, lease, observed) = checkpoint.into_parts();
        if tokens.scope() != scope {
            return None;
        }
        Some(Self {
            tokens,
            lease,
            observed,
            clock,
        })
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

    /// The scope carried by every token this authority issues.
    #[must_use]
    pub const fn scope(&self) -> u16 {
        self.tokens.scope()
    }

    /// How many tokens have been issued; also the sequence number of the last.
    #[must_use]
    pub const fn issued(&self) -> u64 {
        self.tokens.issued()
    }

    /// Capture state for a host-controlled commit or deterministic simulation.
    /// Persist it before exposing the decision that produced it.
    #[must_use]
    pub const fn checkpoint(&self) -> Checkpoint {
        Checkpoint::committed(self.tokens, self.lease, self.observed)
    }

    /// Acquire an unoccupied or expired resource with a new, strictly greater
    /// token.
    ///
    /// # Errors
    /// `ClockRegressed`, `InvalidTtl`, `Occupied` (another owner's unexpired
    /// grant), `Held` (the owner's own unexpired grant) or `Exhausted`.
    pub fn acquire(
        &mut self,
        owner: OwnerId,
        now: Duration,
        ttl: Duration,
    ) -> Result<Lease, Refusal> {
        self.grant(owner, now, ttl, false)
    }

    /// Acquire like `acquire`, or rotate the token of `owner`'s own unexpired
    /// grant.
    ///
    /// The previous token becomes stale, so in-flight work under it is
    /// fenced. Every honoured call issues a new token: a repeat rotates again.
    ///
    /// # Errors
    /// `ClockRegressed`, `InvalidTtl`, `Occupied` or `Exhausted`.
    pub fn reacquire(
        &mut self,
        owner: OwnerId,
        now: Duration,
        ttl: Duration,
    ) -> Result<Lease, Refusal> {
        self.grant(owner, now, ttl, true)
    }

    fn grant(
        &mut self,
        owner: OwnerId,
        now: Duration,
        ttl: Duration,
        rotate: bool,
    ) -> Result<Lease, Refusal> {
        let now = self.at(now)?;
        let expires_at = expiration(now, ttl).ok_or(Refusal::InvalidTtl)?;
        if let Some(lease) = self.lease
            && now < lease.expires_at()
        {
            if lease.owner() != owner {
                return Err(Refusal::Occupied);
            }
            if !rotate {
                return Err(Refusal::Held);
            }
        }
        // Issuing is the last thing that can refuse, so no token is consumed
        // by a call refused for another reason.
        let token = match self.tokens.issue() {
            Ok(raw) => FencingToken::issued(raw),
            Err(periscope_machines::Refusal::Exhausted) => return Err(Refusal::Exhausted),
        };
        let lease = Lease::new(owner, token, expires_at);
        self.lease = Some(lease);
        self.observed = now;
        Ok(lease)
    }

    /// Renew the current grant, retaining its token, when it is unexpired at
    /// the instant the call is evaluated at.
    ///
    /// A shorter requested TTL does not shorten the previous expiration, so a
    /// repeated or reordered renewal is honoured and never loses time.
    ///
    /// # Errors
    /// `Unknown`, `ClockRegressed`, `NoLease`, `WrongOwner`, `StaleToken`,
    /// `Expired` or `InvalidTtl`.
    pub fn renew(
        &mut self,
        owner: OwnerId,
        token: FencingToken,
        now: Duration,
        ttl: Duration,
    ) -> Result<Lease, Refusal> {
        let (now, lease) = self.held(owner, token, now)?;
        let expires_at = expiration(now, ttl).ok_or(Refusal::InvalidTtl)?;
        let lease = Lease::new(owner, token, lease.expires_at().max(expires_at));
        self.lease = Some(lease);
        self.observed = now;
        Ok(lease)
    }

    /// Release the current grant, when it is unexpired at the instant the
    /// call is evaluated at, answering the grant that was released. The token
    /// count is retained: the next token is still greater.
    ///
    /// A repeat is `NoLease`, not an honoured duplicate: the authority keeps
    /// no record of who released what, so it cannot prove the caller's claim.
    ///
    /// # Errors
    /// `Unknown`, `ClockRegressed`, `NoLease`, `WrongOwner`, `StaleToken` or
    /// `Expired`.
    pub fn release(
        &mut self,
        owner: OwnerId,
        token: FencingToken,
        now: Duration,
    ) -> Result<Lease, Refusal> {
        let (now, lease) = self.held(owner, token, now)?;
        self.lease = None;
        self.observed = now;
        Ok(lease)
    }

    /// The instant to evaluate at and the current grant, when `owner` holds
    /// it under `token` and it has not expired. Records nothing.
    fn held(
        &self,
        owner: OwnerId,
        token: FencingToken,
        now: Duration,
    ) -> Result<(Duration, Lease), Refusal> {
        if !self.tokens.owns(token.raw()) {
            return Err(Refusal::Unknown);
        }
        let now = self.at(now)?;
        let lease = self.lease.ok_or(Refusal::NoLease)?;
        if lease.owner() != owner {
            Err(Refusal::WrongOwner)
        } else if lease.token() != token {
            Err(Refusal::StaleToken)
        } else if now >= lease.expires_at() {
            Err(Refusal::Expired)
        } else {
            Ok((now, lease))
        }
    }

    /// The instant a request at `now` is evaluated at. Records nothing: the
    /// watermark moves only when the call is honoured.
    fn at(&self, now: Duration) -> Result<Duration, Refusal> {
        if now >= self.observed {
            return Ok(now);
        }
        match self.clock {
            ClockPolicy::Reject => Err(Refusal::ClockRegressed),
            ClockPolicy::Clamp => Ok(self.observed),
        }
    }
}

const fn expiration(now: Duration, ttl: Duration) -> Option<Duration> {
    if ttl.is_zero() {
        None
    } else {
        now.checked_add(ttl)
    }
}

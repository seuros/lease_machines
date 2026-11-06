use core::time::Duration;

use crate::FencingToken;

/// Renewal timing for one grant, on the holder's own clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cadence {
    ttl: Duration,
    renew_after: Duration,
    warn_before: Duration,
}

impl Cadence {
    /// Renew `renew_after` each confirmation; warn `warn_before` the local expiry.
    ///
    /// Returns `None` unless `renew_after < ttl` and `warn_before <= ttl`.
    #[must_use]
    pub const fn new(ttl: Duration, renew_after: Duration, warn_before: Duration) -> Option<Self> {
        if ttl.is_zero()
            || renew_after.as_nanos() >= ttl.as_nanos()
            || warn_before.as_nanos() > ttl.as_nanos()
        {
            return None;
        }
        Some(Self {
            ttl,
            renew_after,
            warn_before,
        })
    }

    /// Lease duration the holder requests on every renewal.
    #[must_use]
    pub const fn ttl(self) -> Duration {
        self.ttl
    }
}

/// Where a holder stands with the grant it believes it owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HolderState {
    /// The last renewal succeeded and none is in flight.
    Confirmed,
    /// A renewal is in flight, with time to spare before the warning deadline.
    Unconfirmed,
    /// A renewal is in flight past the warning deadline.
    AtRisk,
    /// The authority reported expiry; only a fresh grant restores ownership.
    Expired,
    /// Ownership is lost for good; no event leaves this state.
    Fenced,
}

/// Holder-side renewal tracker for a single grant.
///
/// Durations come from the holder's clock, not the authority's, so two
/// machines never compare instants. Deadlines are derived from the moment a
/// renewal was confirmed locally, which is never later than the authority's view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Holder {
    state: HolderState,
    cadence: Cadence,
    token: FencingToken,
    renew_due: Duration,
    warning_at: Duration,
}

impl Holder {
    /// Track a grant confirmed at `confirmed_at` on the holder's clock.
    #[must_use]
    pub const fn new(token: FencingToken, confirmed_at: Duration, cadence: Cadence) -> Self {
        let mut holder = Self {
            state: HolderState::Confirmed,
            cadence,
            token,
            renew_due: Duration::ZERO,
            warning_at: Duration::ZERO,
        };
        holder.schedule(confirmed_at);
        holder
    }

    /// Current lifecycle state.
    #[must_use]
    pub const fn state(&self) -> HolderState {
        self.state
    }

    /// Newest token this holder has seen for its grant.
    #[must_use]
    pub const fn token(&self) -> FencingToken {
        self.token
    }

    /// Whether a renewal should be started or retried now.
    #[must_use]
    pub const fn renewal_due(&self, now: Duration) -> bool {
        match self.state {
            HolderState::Confirmed => now.as_nanos() >= self.renew_due.as_nanos(),
            HolderState::Fenced => false,
            HolderState::Unconfirmed | HolderState::AtRisk | HolderState::Expired => true,
        }
    }

    /// Point after which unconfirmed ownership should be treated as doubtful.
    #[must_use]
    pub const fn warning_deadline(&self) -> Option<Duration> {
        match self.state {
            HolderState::Fenced => None,
            _ => Some(self.warning_at),
        }
    }

    /// Whether ownership is unconfirmed or the warning deadline has passed.
    #[must_use]
    pub const fn at_risk(&self, now: Duration) -> bool {
        !matches!(self.state, HolderState::Confirmed)
            || now.as_nanos() >= self.warning_at.as_nanos()
    }

    /// Start a renewal attempt; repeated calls while one is outstanding are no-ops.
    ///
    /// Returns `false` once fenced.
    pub const fn refresh(&mut self, now: Duration) -> bool {
        match self.state {
            HolderState::Fenced => false,
            HolderState::Confirmed | HolderState::Unconfirmed => {
                self.state = if now.as_nanos() >= self.warning_at.as_nanos() {
                    HolderState::AtRisk
                } else {
                    HolderState::Unconfirmed
                };
                true
            }
            HolderState::AtRisk | HolderState::Expired => true,
        }
    }

    /// Record a successful renewal observed at `at`.
    ///
    /// A response carrying a token older than the current one is ignored, as is
    /// one arriving without an outstanding attempt or after expiry or fencing.
    pub const fn confirm(&mut self, token: FencingToken, at: Duration) -> bool {
        match self.state {
            HolderState::Unconfirmed | HolderState::AtRisk
                if token.value() >= self.token.value() =>
            {
                self.token = token;
                self.state = HolderState::Confirmed;
                self.schedule(at);
                true
            }
            _ => false,
        }
    }

    /// Record that the authority reported the grant expired.
    pub const fn expire(&mut self) -> bool {
        if matches!(self.state, HolderState::Fenced) {
            return false;
        }
        self.state = HolderState::Expired;
        true
    }

    /// Record a fresh grant with a strictly newer token after expiry.
    pub const fn reacquire(&mut self, token: FencingToken, at: Duration) -> bool {
        match self.state {
            HolderState::Expired if token.value() > self.token.value() => {
                self.token = token;
                self.state = HolderState::Confirmed;
                self.schedule(at);
                true
            }
            _ => false,
        }
    }

    /// Give up ownership permanently; idempotent.
    pub const fn fence(&mut self) {
        self.state = HolderState::Fenced;
    }

    const fn schedule(&mut self, confirmed_at: Duration) {
        self.renew_due = confirmed_at.saturating_add(self.cadence.renew_after);
        let window = self.cadence.ttl.saturating_sub(self.cadence.warn_before);
        self.warning_at = confirmed_at.saturating_add(window);
    }
}

#[cfg(test)]
mod tests;

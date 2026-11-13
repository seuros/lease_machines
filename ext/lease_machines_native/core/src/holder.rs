use core::time::Duration;

use crate::{FencingToken, Refusal};

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
    /// Returns `None` unless `ttl` is positive, `renew_after < ttl` and
    /// `warn_before <= ttl`.
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

    /// How long after a confirmation the next renewal is due.
    #[must_use]
    pub const fn renew_after(self) -> Duration {
        self.renew_after
    }

    /// How long before the local expiry unconfirmed ownership becomes doubtful.
    #[must_use]
    pub const fn warn_before(self) -> Duration {
        self.warn_before
    }
}

/// Where a holder stands with the grant it believes it owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Honoured outcome of [`Holder::refresh`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refreshing {
    /// A renewal attempt begins now: the holder was confirmed.
    Started,
    /// An attempt was already outstanding, or the grant is expired and only a
    /// fresh one helps. The attempt is not restarted; an unconfirmed holder
    /// past its warning deadline becomes at risk.
    Outstanding,
}

/// Honoured outcome of a report to the holder: [`Holder::confirm`],
/// [`Holder::expire`] and [`Holder::reacquire`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recording {
    /// Recorded for the first time; the holder's state changed.
    Recorded,
    /// Idempotent repeat: what the report says already holds, under the same
    /// token. Nothing changed, not even the renewal schedule.
    Duplicate,
}

/// Holder-side renewal tracker for a single grant.
///
/// Durations come from the holder's clock, not the authority's, so two
/// machines never compare instants. Deadlines are derived from the moment a
/// renewal was confirmed locally, which is never later than the authority's view.
///
/// A holder follows the tokens of one scope: the scope of the token it was
/// built with. A token of another scope is [`Refusal::Unknown`], because it
/// cannot be ranked against the one held; track a grant from another
/// authority with a new holder.
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

    /// The renewal timing this holder was built with.
    #[must_use]
    pub const fn cadence(&self) -> Cadence {
        self.cadence
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
    /// `None` once fenced.
    #[must_use]
    pub const fn warning_deadline(&self) -> Option<Duration> {
        match self.state {
            HolderState::Fenced => None,
            HolderState::Confirmed
            | HolderState::Unconfirmed
            | HolderState::AtRisk
            | HolderState::Expired => Some(self.warning_at),
        }
    }

    /// Whether ownership is unconfirmed or the warning deadline has passed.
    #[must_use]
    pub const fn at_risk(&self, now: Duration) -> bool {
        match self.state {
            HolderState::Confirmed => now.as_nanos() >= self.warning_at.as_nanos(),
            HolderState::Unconfirmed
            | HolderState::AtRisk
            | HolderState::Expired
            | HolderState::Fenced => true,
        }
    }

    /// Start a renewal attempt, or learn that one is already outstanding.
    ///
    /// # Errors
    /// `Fenced`. Nothing changes.
    pub const fn refresh(&mut self, now: Duration) -> Result<Refreshing, Refusal> {
        let late = now.as_nanos() >= self.warning_at.as_nanos();
        let pending = if late {
            HolderState::AtRisk
        } else {
            HolderState::Unconfirmed
        };
        match self.state {
            HolderState::Fenced => Err(Refusal::Fenced),
            HolderState::Confirmed => {
                self.state = pending;
                Ok(Refreshing::Started)
            }
            HolderState::Unconfirmed => {
                self.state = pending;
                Ok(Refreshing::Outstanding)
            }
            HolderState::AtRisk | HolderState::Expired => Ok(Refreshing::Outstanding),
        }
    }

    /// Record a successful renewal observed at `at`, under `token`: the one
    /// held, or a newer one of its scope.
    ///
    /// A confirmation of the token already held while no renewal is
    /// outstanding is a [`Recording::Duplicate`]: the holder is confirmed
    /// under that token, which is all the report says. `at` is when the
    /// report arrived, not part of it, and a duplicate does not reschedule.
    ///
    /// # Errors
    /// `Fenced`; `Unknown` (another scope); `StaleToken` (older than the one
    /// held: a late reply); `OutOfPhase` (a newer token with no renewal
    /// outstanding); `Expired` (only `reacquire` recovers).
    pub const fn confirm(
        &mut self,
        token: FencingToken,
        at: Duration,
    ) -> Result<Recording, Refusal> {
        let same = match self.rank(token) {
            Ok(same) => same,
            Err(refusal) => return Err(refusal),
        };
        match self.state {
            HolderState::Unconfirmed | HolderState::AtRisk => {
                self.accept(token, at);
                Ok(Recording::Recorded)
            }
            HolderState::Confirmed if same => Ok(Recording::Duplicate),
            HolderState::Confirmed => Err(Refusal::OutOfPhase),
            HolderState::Expired => Err(Refusal::Expired),
            // Unreachable: `rank` refuses a fenced holder.
            HolderState::Fenced => Err(Refusal::Fenced),
        }
    }

    /// Record that the authority reported the grant expired.
    ///
    /// # Errors
    /// `Fenced`. Nothing changes.
    pub const fn expire(&mut self) -> Result<Recording, Refusal> {
        match self.state {
            HolderState::Fenced => Err(Refusal::Fenced),
            HolderState::Expired => Ok(Recording::Duplicate),
            HolderState::Confirmed | HolderState::Unconfirmed | HolderState::AtRisk => {
                self.state = HolderState::Expired;
                Ok(Recording::Recorded)
            }
        }
    }

    /// Record a fresh grant with a strictly newer token after expiry.
    ///
    /// The same report again, once the holder is confirmed under `token`, is
    /// a [`Recording::Duplicate`] and does not reschedule.
    ///
    /// # Errors
    /// `Fenced`; `Unknown` (another scope); `StaleToken` (older than the one
    /// held, or the expired grant's own token); `OutOfPhase` (the grant is
    /// not expired and `token` is not the one held and confirmed).
    pub const fn reacquire(
        &mut self,
        token: FencingToken,
        at: Duration,
    ) -> Result<Recording, Refusal> {
        let same = match self.rank(token) {
            Ok(same) => same,
            Err(refusal) => return Err(refusal),
        };
        match self.state {
            HolderState::Expired if same => Err(Refusal::StaleToken),
            HolderState::Expired => {
                self.accept(token, at);
                Ok(Recording::Recorded)
            }
            HolderState::Confirmed if same => Ok(Recording::Duplicate),
            HolderState::Confirmed | HolderState::Unconfirmed | HolderState::AtRisk => {
                Err(Refusal::OutOfPhase)
            }
            // Unreachable: `rank` refuses a fenced holder.
            HolderState::Fenced => Err(Refusal::Fenced),
        }
    }

    /// Give up ownership permanently. It cannot refuse, and fencing a fenced
    /// holder changes nothing.
    pub const fn fence(&mut self) {
        self.state = HolderState::Fenced;
    }

    /// Whether `token` is the one held (`true`) or a newer one of its scope
    /// (`false`); the refusal every token-carrying report shares otherwise.
    const fn rank(&self, token: FencingToken) -> Result<bool, Refusal> {
        if matches!(self.state, HolderState::Fenced) {
            return Err(Refusal::Fenced);
        }
        if token.scope() != self.token.scope() {
            return Err(Refusal::Unknown);
        }
        // Same scope on both sides, so the raw numbers rank them.
        if token.raw() < self.token.raw() {
            return Err(Refusal::StaleToken);
        }
        Ok(token.raw() == self.token.raw())
    }

    const fn accept(&mut self, token: FencingToken, at: Duration) {
        self.token = token;
        self.state = HolderState::Confirmed;
        self.schedule(at);
    }

    const fn schedule(&mut self, confirmed_at: Duration) {
        self.renew_due = confirmed_at.saturating_add(self.cadence.renew_after);
        let window = self.cadence.ttl.saturating_sub(self.cadence.warn_before);
        self.warning_at = confirmed_at.saturating_add(window);
    }
}

//! Naive reference models. The authority keeps its whole history and derives
//! the watermark and the live grant from it; the fence keeps every token it
//! accepted; the holder keeps the last confirmation, derives its deadlines,
//! and answers every report from a transition table written cell by cell,
//! not from the core's code restated.
use std::vec::Vec;

use periscope_machines::{SEQUENCE_MAX, scope_of, sequence_of};

use super::*;

struct Grant {
    owner: OwnerId,
    sequence: u64,
    expires_at: Duration,
    released: bool,
}

pub struct Authority {
    pub scope: u16,
    /// Tokens the history had issued before the model began.
    pub base: u64,
    pub clamp: bool,
    grants: Vec<Grant>,
    /// The instant every honoured call was evaluated at, in order.
    honoured: Vec<Duration>,
}

impl Authority {
    pub const fn new(scope: u16, base: u64, clamp: bool) -> Self {
        Self {
            scope,
            base,
            clamp,
            grants: Vec::new(),
            honoured: Vec::new(),
        }
    }

    pub fn issued(&self) -> u64 {
        self.base + self.grants.len() as u64
    }

    pub fn observed(&self) -> Duration {
        self.honoured.iter().copied().max().unwrap_or_default()
    }

    /// The grant the history ends with, expired or not, unless released.
    fn last(&self) -> Option<&Grant> {
        self.grants.last().filter(|grant| !grant.released)
    }

    pub fn lease(&self) -> Option<Lease> {
        self.last().map(|grant| {
            Lease::new(
                grant.owner,
                token(self.scope, grant.sequence),
                grant.expires_at,
            )
        })
    }

    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint::from_parts(self.scope, self.issued(), self.lease(), self.observed()).unwrap()
    }

    fn evaluated(&self, now: Duration) -> Result<Duration, Refusal> {
        match (now < self.observed(), self.clamp) {
            (false, _) => Ok(now),
            (true, true) => Ok(self.observed()),
            (true, false) => Err(Refusal::ClockRegressed),
        }
    }

    fn owns(&self, token: FencingToken) -> bool {
        let sequence = sequence_of(token.raw());
        scope_of(token.raw()) == self.scope && (1..=self.issued()).contains(&sequence)
    }

    /// The live grant a grant at `now` would meet, under the same checks.
    pub fn incumbent(&self, now: Duration, ttl: Duration) -> Result<Option<Lease>, Refusal> {
        let now = self.evaluated(now)?;
        if ttl.is_zero() || now.checked_add(ttl).is_none() {
            return Err(Refusal::InvalidTtl);
        }
        let live = self.last().filter(|grant| now < grant.expires_at);
        Ok(live.and_then(|_| self.lease()))
    }

    pub fn grant(
        &mut self,
        owner: OwnerId,
        now: Duration,
        ttl: Duration,
        rotate: bool,
    ) -> Result<Lease, Refusal> {
        let now = self.evaluated(now)?;
        if ttl.is_zero() {
            return Err(Refusal::InvalidTtl);
        }
        let expires_at = now.checked_add(ttl).ok_or(Refusal::InvalidTtl)?;
        if let Some(live) = self.last().filter(|grant| now < grant.expires_at) {
            if live.owner != owner {
                return Err(Refusal::Occupied);
            }
            if !rotate {
                return Err(Refusal::Held);
            }
        }
        if self.issued() == SEQUENCE_MAX {
            return Err(Refusal::Exhausted);
        }
        let sequence = self.issued() + 1;
        self.grants.push(Grant {
            owner,
            sequence,
            expires_at,
            released: false,
        });
        self.honoured.push(now);
        Ok(self.lease().unwrap())
    }

    fn held(
        &self,
        owner: OwnerId,
        token: FencingToken,
        now: Duration,
    ) -> Result<Duration, Refusal> {
        if !self.owns(token) {
            return Err(Refusal::Unknown);
        }
        let now = self.evaluated(now)?;
        let live = self.last().ok_or(Refusal::NoLease)?;
        if live.owner != owner {
            return Err(Refusal::WrongOwner);
        }
        if live.sequence != sequence_of(token.raw()) {
            return Err(Refusal::StaleToken);
        }
        if now >= live.expires_at {
            return Err(Refusal::Expired);
        }
        Ok(now)
    }

    pub fn renew(
        &mut self,
        owner: OwnerId,
        token: FencingToken,
        now: Duration,
        ttl: Duration,
    ) -> Result<Lease, Refusal> {
        let now = self.held(owner, token, now)?;
        if ttl.is_zero() {
            return Err(Refusal::InvalidTtl);
        }
        let asked = now.checked_add(ttl).ok_or(Refusal::InvalidTtl)?;
        let live = self.grants.last_mut().unwrap();
        live.expires_at = live.expires_at.max(asked);
        self.honoured.push(now);
        Ok(self.lease().unwrap())
    }

    pub fn release(
        &mut self,
        owner: OwnerId,
        token: FencingToken,
        now: Duration,
    ) -> Result<Lease, Refusal> {
        let now = self.held(owner, token, now)?;
        let lease = self.lease().unwrap();
        self.grants.last_mut().unwrap().released = true;
        self.honoured.push(now);
        Ok(lease)
    }
}

/// Every token the resource accepted, in order; the high-water mark is the
/// greatest of them.
pub struct Resource {
    pub scope: u16,
    pub accepted: Vec<u64>,
}

impl Resource {
    pub fn highest(&self) -> Option<FencingToken> {
        let highest = self.accepted.iter().copied().max()?;
        Some(token(self.scope, highest))
    }

    pub fn accept(&mut self, token: FencingToken) -> Result<Accepting, Refusal> {
        if token.scope() != self.scope {
            return Err(Refusal::Unknown);
        }
        let sequence = sequence_of(token.raw());
        if self.accepted.iter().any(|seen| *seen > sequence) {
            return Err(Refusal::StaleToken);
        }
        if self.accepted.contains(&sequence) {
            return Ok(Accepting::Current);
        }
        self.accepted.push(sequence);
        Ok(Accepting::Advanced)
    }
}

/// How a reported token stands against the one the holder has.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Foreign,
    Older,
    Same,
    Newer,
}

/// What a report does to the holder, beside its answer.
#[derive(Clone, Copy)]
enum Then {
    /// Nothing changes.
    Stay,
    /// Waiting for the authority: `AtRisk` past the warning, else `Unconfirmed`.
    Pending,
    /// The authority said the grant is gone.
    Expire,
    /// Confirmed under the reported token, rescheduled from the report.
    Confirm,
}

use HolderState::{AtRisk, Confirmed, Expired, Fenced, Unconfirmed};
use Kind::{Foreign, Newer, Older, Same};

/// `refresh`, one row per state.
const REFRESH: [(HolderState, Result<Refreshing, Refusal>, Then); 5] = [
    (Confirmed, Ok(Refreshing::Started), Then::Pending),
    (Unconfirmed, Ok(Refreshing::Outstanding), Then::Pending),
    (AtRisk, Ok(Refreshing::Outstanding), Then::Stay),
    (Expired, Ok(Refreshing::Outstanding), Then::Stay),
    (Fenced, Err(Refusal::Fenced), Then::Stay),
];

/// `expire`, one row per state.
const EXPIRE: [(HolderState, Result<Recording, Refusal>, Then); 5] = [
    (Confirmed, Ok(Recording::Recorded), Then::Expire),
    (Unconfirmed, Ok(Recording::Recorded), Then::Expire),
    (AtRisk, Ok(Recording::Recorded), Then::Expire),
    (Expired, Ok(Recording::Duplicate), Then::Stay),
    (Fenced, Err(Refusal::Fenced), Then::Stay),
];

/// A report carrying a token: one row per state and kind of token, every
/// cell written out.
type Reported = [(HolderState, Kind, Result<Recording, Refusal>, Then); 20];

const RECORDED: Result<Recording, Refusal> = Ok(Recording::Recorded);
const DUPLICATE: Result<Recording, Refusal> = Ok(Recording::Duplicate);
const FENCED: Result<Recording, Refusal> = Err(Refusal::Fenced);
const UNKNOWN: Result<Recording, Refusal> = Err(Refusal::Unknown);
const STALE: Result<Recording, Refusal> = Err(Refusal::StaleToken);
const OUT_OF_PHASE: Result<Recording, Refusal> = Err(Refusal::OutOfPhase);
const EXPIRED: Result<Recording, Refusal> = Err(Refusal::Expired);

/// `confirm`: the renewal it waited for, under the token held or a newer one.
const CONFIRM: Reported = [
    (Confirmed, Foreign, UNKNOWN, Then::Stay),
    (Confirmed, Older, STALE, Then::Stay),
    (Confirmed, Same, DUPLICATE, Then::Stay),
    (Confirmed, Newer, OUT_OF_PHASE, Then::Stay),
    (Unconfirmed, Foreign, UNKNOWN, Then::Stay),
    (Unconfirmed, Older, STALE, Then::Stay),
    (Unconfirmed, Same, RECORDED, Then::Confirm),
    (Unconfirmed, Newer, RECORDED, Then::Confirm),
    (AtRisk, Foreign, UNKNOWN, Then::Stay),
    (AtRisk, Older, STALE, Then::Stay),
    (AtRisk, Same, RECORDED, Then::Confirm),
    (AtRisk, Newer, RECORDED, Then::Confirm),
    (Expired, Foreign, UNKNOWN, Then::Stay),
    (Expired, Older, STALE, Then::Stay),
    (Expired, Same, EXPIRED, Then::Stay),
    (Expired, Newer, EXPIRED, Then::Stay),
    (Fenced, Foreign, FENCED, Then::Stay),
    (Fenced, Older, FENCED, Then::Stay),
    (Fenced, Same, FENCED, Then::Stay),
    (Fenced, Newer, FENCED, Then::Stay),
];

/// `reacquire`: a fresh grant after expiry, under a strictly newer token.
const REACQUIRE: Reported = [
    (Confirmed, Foreign, UNKNOWN, Then::Stay),
    (Confirmed, Older, STALE, Then::Stay),
    (Confirmed, Same, DUPLICATE, Then::Stay),
    (Confirmed, Newer, OUT_OF_PHASE, Then::Stay),
    (Unconfirmed, Foreign, UNKNOWN, Then::Stay),
    (Unconfirmed, Older, STALE, Then::Stay),
    (Unconfirmed, Same, OUT_OF_PHASE, Then::Stay),
    (Unconfirmed, Newer, OUT_OF_PHASE, Then::Stay),
    (AtRisk, Foreign, UNKNOWN, Then::Stay),
    (AtRisk, Older, STALE, Then::Stay),
    (AtRisk, Same, OUT_OF_PHASE, Then::Stay),
    (AtRisk, Newer, OUT_OF_PHASE, Then::Stay),
    (Expired, Foreign, UNKNOWN, Then::Stay),
    (Expired, Older, STALE, Then::Stay),
    (Expired, Same, STALE, Then::Stay),
    (Expired, Newer, RECORDED, Then::Confirm),
    (Fenced, Foreign, FENCED, Then::Stay),
    (Fenced, Older, FENCED, Then::Stay),
    (Fenced, Same, FENCED, Then::Stay),
    (Fenced, Newer, FENCED, Then::Stay),
];

/// The holder as a state, a token and a confirmation time, moved only by
/// looking each report up in the tables above.
pub struct Tracker {
    pub state: HolderState,
    pub token: FencingToken,
    pub cadence: Cadence,
    /// When the grant was last confirmed, on the holder's clock.
    pub confirmed_at: Duration,
}

impl Tracker {
    pub const fn new(token: FencingToken, confirmed_at: Duration, cadence: Cadence) -> Self {
        Self {
            state: HolderState::Confirmed,
            token,
            cadence,
            confirmed_at,
        }
    }

    fn warning_at(&self) -> Duration {
        let window = self
            .cadence
            .ttl()
            .saturating_sub(self.cadence.warn_before());
        self.confirmed_at.saturating_add(window)
    }

    pub fn warning_deadline(&self) -> Option<Duration> {
        (self.state != HolderState::Fenced).then(|| self.warning_at())
    }

    pub fn renewal_due(&self, now: Duration) -> bool {
        match self.state {
            HolderState::Confirmed => {
                now >= self.confirmed_at.saturating_add(self.cadence.renew_after())
            }
            HolderState::Fenced => false,
            HolderState::Unconfirmed | HolderState::AtRisk | HolderState::Expired => true,
        }
    }

    pub fn at_risk(&self, now: Duration) -> bool {
        self.state != HolderState::Confirmed || now >= self.warning_at()
    }

    fn kind(&self, token: FencingToken) -> Kind {
        if token.scope() != self.token.scope() {
            Foreign
        } else if token.raw() < self.token.raw() {
            Older
        } else if token == self.token {
            Same
        } else {
            Newer
        }
    }

    fn apply(&mut self, then: Then, now: Duration, token: FencingToken) {
        match then {
            Then::Stay => {}
            Then::Pending if now >= self.warning_at() => self.state = AtRisk,
            Then::Pending => self.state = Unconfirmed,
            Then::Expire => self.state = Expired,
            Then::Confirm => {
                self.state = Confirmed;
                self.token = token;
                self.confirmed_at = now;
            }
        }
    }

    fn reported(
        &mut self,
        table: &Reported,
        token: FencingToken,
        at: Duration,
    ) -> Result<Recording, Refusal> {
        let kind = self.kind(token);
        let row = table
            .iter()
            .find(|row| (row.0, row.1) == (self.state, kind));
        let &(_, _, answer, then) = row.unwrap();
        self.apply(then, at, token);
        answer
    }

    pub fn refresh(&mut self, now: Duration) -> Result<Refreshing, Refusal> {
        let &(_, answer, then) = REFRESH.iter().find(|row| row.0 == self.state).unwrap();
        self.apply(then, now, self.token);
        answer
    }

    pub fn confirm(&mut self, token: FencingToken, at: Duration) -> Result<Recording, Refusal> {
        self.reported(&CONFIRM, token, at)
    }

    pub fn expire(&mut self) -> Result<Recording, Refusal> {
        let &(_, answer, then) = EXPIRE.iter().find(|row| row.0 == self.state).unwrap();
        self.apply(then, self.confirmed_at, self.token);
        answer
    }

    pub fn reacquire(&mut self, token: FencingToken, at: Duration) -> Result<Recording, Refusal> {
        self.reported(&REACQUIRE, token, at)
    }

    pub const fn fence(&mut self) {
        self.state = HolderState::Fenced;
    }
}

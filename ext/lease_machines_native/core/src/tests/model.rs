//! Naive reference models. The authority keeps its whole history and derives
//! the watermark and the live grant from it; the fence keeps every token it
//! accepted; the holder keeps the last confirmation and derives its deadlines.
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
enum Kind {
    Foreign,
    Older,
    Same,
    Newer,
}

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
            Kind::Foreign
        } else if token.raw() < self.token.raw() {
            Kind::Older
        } else if token == self.token {
            Kind::Same
        } else {
            Kind::Newer
        }
    }

    const fn confirmed(&mut self, token: FencingToken, at: Duration) -> Recording {
        self.token = token;
        self.confirmed_at = at;
        self.state = HolderState::Confirmed;
        Recording::Recorded
    }

    pub fn refresh(&mut self, now: Duration) -> Result<Refreshing, Refusal> {
        let pending = if now >= self.warning_at() {
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

    pub fn confirm(&mut self, token: FencingToken, at: Duration) -> Result<Recording, Refusal> {
        use HolderState::{AtRisk, Confirmed, Expired, Fenced, Unconfirmed};
        match (self.state, self.kind(token)) {
            (Fenced, _) => Err(Refusal::Fenced),
            (_, Kind::Foreign) => Err(Refusal::Unknown),
            (_, Kind::Older) => Err(Refusal::StaleToken),
            (Unconfirmed | AtRisk, Kind::Same | Kind::Newer) => Ok(self.confirmed(token, at)),
            (Confirmed, Kind::Same) => Ok(Recording::Duplicate),
            (Confirmed, Kind::Newer) => Err(Refusal::OutOfPhase),
            (Expired, Kind::Same | Kind::Newer) => Err(Refusal::Expired),
        }
    }

    pub fn expire(&mut self) -> Result<Recording, Refusal> {
        match self.state {
            HolderState::Fenced => Err(Refusal::Fenced),
            HolderState::Expired => Ok(Recording::Duplicate),
            HolderState::Confirmed | HolderState::Unconfirmed | HolderState::AtRisk => {
                self.state = HolderState::Expired;
                Ok(Recording::Recorded)
            }
        }
    }

    pub fn reacquire(&mut self, token: FencingToken, at: Duration) -> Result<Recording, Refusal> {
        use HolderState::{AtRisk, Confirmed, Expired, Fenced, Unconfirmed};
        match (self.state, self.kind(token)) {
            (Fenced, _) => Err(Refusal::Fenced),
            (_, Kind::Foreign) => Err(Refusal::Unknown),
            (_, Kind::Older) | (Expired, Kind::Same) => Err(Refusal::StaleToken),
            (Expired, Kind::Newer) => Ok(self.confirmed(token, at)),
            (Confirmed, Kind::Same) => Ok(Recording::Duplicate),
            (Confirmed, Kind::Newer) | (Unconfirmed | AtRisk, Kind::Same | Kind::Newer) => {
                Err(Refusal::OutOfPhase)
            }
        }
    }

    pub const fn fence(&mut self) {
        self.state = HolderState::Fenced;
    }
}

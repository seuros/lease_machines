//! Randomized comparison of authority, fence and holder against the naive
//! models, in non-zero scopes, with foreign and forged tokens, regressing
//! clocks and a restore from checkpoint in the middle of the run.
use std::{collections::BTreeSet, format, string::String, vec::Vec};

use periscope_machines::{SEQUENCE_MAX, sequence_of};

use super::model::{Authority, Resource, Tracker};
use super::*;

mod holding;

struct Draws(u64);

impl Draws {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

struct Run {
    draws: Draws,
    core: LeaseAuthority,
    model: Authority,
    fence: Fence,
    resource: Resource,
    /// Owner zero's renewal tracker and its model, once it holds a grant.
    holder: Option<(Holder, Tracker)>,
    /// An authority of another scope: where the foreign tokens come from.
    rival: LeaseAuthority,
    now: Duration,
    /// Every token this run's authority issued.
    issued: Vec<FencingToken>,
    foreign: Vec<FencingToken>,
    seen: BTreeSet<String>,
}

fn label<T>(answer: &Result<T, Refusal>) -> String {
    answer
        .as_ref()
        .map_or_else(|refusal| format!("{refusal:?}"), |_| String::from("Ok"))
}

impl Run {
    fn new(seed: u64) -> Self {
        let mut draws = Draws(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let scope = u16::try_from(1 + draws.below(u64::from(u16::MAX))).unwrap();
        let other = if scope == u16::MAX { 1 } else { scope + 1 };
        // Some histories begin a few tokens short of the end of their scope.
        let base = if seed.is_multiple_of(5) {
            SEQUENCE_MAX - 6
        } else {
            draws.below(1000)
        };
        let clamp = seed.is_multiple_of(2);
        let policy = if clamp {
            ClockPolicy::Clamp
        } else {
            ClockPolicy::Reject
        };
        let saved = Checkpoint::from_parts(scope, base, None, Duration::ZERO).unwrap();
        Self {
            draws,
            core: LeaseAuthority::restored(scope, saved, policy).unwrap(),
            model: Authority::new(scope, base, clamp),
            fence: Fence::with_scope(scope),
            resource: Resource {
                scope,
                accepted: Vec::new(),
            },
            holder: None,
            rival: LeaseAuthority::with_scope(other),
            now: at(1),
            issued: Vec::new(),
            foreign: Vec::new(),
            seen: BTreeSet::new(),
        }
    }

    fn owner(&mut self) -> OwnerId {
        OwnerId::new(self.draws.below(3))
    }

    /// The instant a call brings: usually the present, sometimes the past.
    fn instant(&mut self) -> Duration {
        if self.draws.chance(15) {
            return self.now.saturating_sub(at(1 + self.draws.below(4)));
        }
        self.now
    }

    fn ttl(&mut self) -> Duration {
        match self.draws.below(20) {
            0 => Duration::ZERO,
            1 => Duration::MAX,
            span => at(span),
        }
    }

    /// A token for the authority or the holder: live, old, never issued, or
    /// of the rival's scope.
    fn token(&mut self) -> FencingToken {
        let scope = self.model.scope;
        match self.draws.below(10) {
            0 => token(
                scope,
                (self.model.issued() + 1 + self.draws.below(3)).min(SEQUENCE_MAX),
            ),
            1 if !self.foreign.is_empty() => self.pick_foreign(),
            2 => token(
                self.rival.scope(),
                1 + self.draws.below(self.model.issued().max(1)),
            ),
            3 | 4 if !self.issued.is_empty() => {
                let index = self.draws.below(self.issued.len() as u64);
                self.issued[usize::try_from(index).unwrap()]
            }
            _ => self
                .model
                .lease()
                .map_or_else(|| token(scope, self.model.issued().max(1)), Lease::token),
        }
    }

    /// A token for a report to the holder: one some authority really issued.
    fn reported(&mut self) -> FencingToken {
        if self.draws.chance(20) && !self.foreign.is_empty() {
            return self.pick_foreign();
        }
        let index = self.draws.below(self.issued.len() as u64);
        self.issued[usize::try_from(index).unwrap()]
    }

    fn pick_foreign(&mut self) -> FencingToken {
        let index = self.draws.below(self.foreign.len() as u64);
        self.foreign[usize::try_from(index).unwrap()]
    }

    /// One authority call against the model, clone-compared when refused.
    fn decide(
        &mut self,
        name: &str,
        call: impl Fn(&mut LeaseAuthority) -> Result<Lease, Refusal>,
        expected: Result<Lease, Refusal>,
    ) -> Result<Lease, Refusal> {
        let before = self.core.clone();
        let answer = call(&mut self.core);
        assert_eq!(answer, expected, "{name}");
        if answer.is_err() {
            assert_eq!(self.core, before, "{name}: a refusal changed something");
        }
        assert_eq!(self.core.checkpoint(), self.model.checkpoint(), "{name}");
        assert_eq!(self.core.issued(), self.model.issued());
        self.seen.insert(format!("{name}:{}", label(&answer)));
        answer
    }

    fn grant(&mut self, owner: OwnerId, rotate: bool) -> Result<Lease, Refusal> {
        let (now, ttl) = (self.instant(), self.ttl());
        let expected = self.model.grant(owner, now, ttl, rotate);
        let name = if rotate { "reacquire" } else { "acquire" };
        let newest = self.issued.last().copied();
        let answer = self.decide(
            name,
            |core| {
                if rotate {
                    core.reacquire(owner, now, ttl)
                } else {
                    core.acquire(owner, now, ttl)
                }
            },
            expected,
        );
        if let Ok(lease) = answer {
            // Newer than everything issued before: fencing depends on it.
            assert!(newest.is_none_or(|old| lease.token().is_newer_than(old)));
            assert_eq!(lease.token().scope(), self.model.scope);
            self.issued.push(lease.token());
        }
        answer
    }

    fn renew(&mut self, owner: OwnerId, token: FencingToken) -> Result<Lease, Refusal> {
        let (now, ttl) = (self.instant(), self.ttl());
        let expected = self.model.renew(owner, token, now, ttl);
        self.decide("renew", |core| core.renew(owner, token, now, ttl), expected)
    }

    fn release(&mut self) {
        let (owner, token, now) = (self.owner(), self.token(), self.instant());
        let expected = self.model.release(owner, token, now);
        let _ = self.decide("release", |core| core.release(owner, token, now), expected);
    }

    /// A write reaches the resource: under a token this authority issued, or
    /// one of the rival's. The fence sees no forged token of its own scope.
    fn write(&mut self) {
        let token = if self.draws.chance(25) && !self.foreign.is_empty() {
            self.pick_foreign()
        } else if let Some(newest) = self.issued.last().copied() {
            if self.draws.chance(50) {
                newest
            } else {
                let index = self.draws.below(self.issued.len() as u64);
                self.issued[usize::try_from(index).unwrap()]
            }
        } else {
            return;
        };
        let before = self.fence.clone();
        let answer = self.fence.accept(token);
        assert_eq!(answer, self.resource.accept(token));
        if answer != Ok(Accepting::Advanced) {
            assert_eq!(self.fence, before, "the fence moved without advancing");
        }
        if self.issued.last() == Some(&token) {
            assert!(answer.is_ok(), "the newest token was fenced out");
        }
        assert_eq!(self.fence.highest(), self.resource.highest());
        self.seen.insert(format!("accept:{answer:?}"));
    }

    /// The process restarts: authority and fence are rebuilt from what was
    /// saved, under the same scope, and carry on as if nothing happened.
    fn restart(&mut self) {
        let (scope, policy) = (self.core.scope(), self.core.clock_policy());
        let wrong = self.rival.scope();
        assert_eq!(
            LeaseAuthority::restored(wrong, self.core.checkpoint(), policy),
            None
        );
        let restored = LeaseAuthority::restored(scope, self.core.checkpoint(), policy).unwrap();
        assert_eq!(restored, self.core);
        self.core = restored;
        let highest = self.fence.highest();
        if highest.is_some() {
            assert_eq!(Fence::resumed(wrong, highest), None);
        }
        self.fence = Fence::resumed(scope, highest).unwrap();
        self.seen.insert(String::from("restart"));
    }

    fn step(&mut self) {
        // Whole seconds, so calls land exactly on deadlines.
        self.now += at(self.draws.below(3));
        match self.draws.below(100) {
            0..=14 => {
                let owner = self.owner();
                let _ = self.grant(owner, false);
            }
            15..=24 => {
                let owner = self.owner();
                let _ = self.grant(owner, true);
            }
            25..=39 => {
                let (owner, token) = (self.owner(), self.token());
                let _ = self.renew(owner, token);
            }
            40..=47 => self.release(),
            48..=62 => self.write(),
            63..=77 => self.report(),
            78..=92 => self.hold(),
            93..=95 => self.restart(),
            _ => {
                let lease = self
                    .rival
                    .reacquire(OwnerId::new(9), self.now, at(5))
                    .unwrap();
                self.foreign.push(lease.token());
            }
        }
        self.check_holder();
        if let Some(highest) = self.fence.highest() {
            // The fence only ever saw this authority's tokens.
            assert!(sequence_of(highest.raw()) <= self.core.issued());
        }
    }
}

/// Every answer the three machines can give; a run of seeds must reach all.
const ANSWERS: [&str; 44] = [
    "accept:Err(StaleToken)",
    "accept:Err(Unknown)",
    "accept:Ok(Advanced)",
    "accept:Ok(Current)",
    "acquire:ClockRegressed",
    "acquire:Exhausted",
    "acquire:Held",
    "acquire:InvalidTtl",
    "acquire:Occupied",
    "acquire:Ok",
    "confirm:Err(Expired)",
    "confirm:Err(Fenced)",
    "confirm:Err(OutOfPhase)",
    "confirm:Err(StaleToken)",
    "confirm:Err(Unknown)",
    "confirm:Ok(Duplicate)",
    "confirm:Ok(Recorded)",
    "expire:Err(Fenced)",
    "expire:Ok(Duplicate)",
    "expire:Ok(Recorded)",
    "reacquire:ClockRegressed",
    "reacquire:Err(Fenced)",
    "reacquire:Err(OutOfPhase)",
    "reacquire:Err(StaleToken)",
    "reacquire:Err(Unknown)",
    "reacquire:Exhausted",
    "reacquire:InvalidTtl",
    "reacquire:Occupied",
    "reacquire:Ok",
    "reacquire:Ok(Duplicate)",
    "reacquire:Ok(Recorded)",
    "refresh:Err(Fenced)",
    "refresh:Ok(Outstanding)",
    "refresh:Ok(Started)",
    "release:ClockRegressed",
    "release:Expired",
    "release:NoLease",
    "release:Ok",
    "release:StaleToken",
    "release:Unknown",
    "release:WrongOwner",
    "renew:Ok",
    "renew:InvalidTtl",
    "restart",
];

#[test]
fn authority_fence_and_holder_agree_with_their_models() {
    let mut seen = BTreeSet::new();
    for seed in 1..=400 {
        let mut run = Run::new(seed);
        for _ in 0..600 {
            run.step();
        }
        seen.append(&mut run.seen);
    }
    let missing: Vec<&str> = ANSWERS
        .into_iter()
        .filter(|answer| !seen.contains(*answer))
        .collect();
    assert!(missing.is_empty(), "never reached: {missing:?}");
    for refusal in [
        "ClockRegressed",
        "Expired",
        "NoLease",
        "StaleToken",
        "Unknown",
        "WrongOwner",
    ] {
        assert!(
            seen.contains(&format!("renew:{refusal}")),
            "renew:{refusal}"
        );
    }
}

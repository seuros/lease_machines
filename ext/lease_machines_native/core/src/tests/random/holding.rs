//! Owner zero's holder: random reports, and the renewal loop it exists for.
use std::format;

use super::Run;
use crate::tests::at;
use crate::tests::model::Tracker;
use crate::{Cadence, Holder, HolderState, OwnerId, Recording, Refusal};

impl Run {
    /// A report reaches owner zero's holder, in any state, with any token.
    pub(super) fn report(&mut self) {
        if self.holder.is_none() {
            return;
        }
        let (token, at) = (self.reported(), self.instant());
        let kind = self.draws.below(20);
        let Some((holder, tracker)) = self.holder.as_mut() else {
            return;
        };
        let before = *holder;
        let (name, answer, expected) = match kind {
            0 => {
                holder.fence();
                tracker.fence();
                return;
            }
            1..=5 => {
                let (answer, expected) = (holder.refresh(at), tracker.refresh(at));
                assert_eq!(answer, expected);
                if answer.is_err() {
                    assert_eq!(*holder, before);
                }
                self.seen.insert(format!("refresh:{answer:?}"));
                return;
            }
            6..=11 => (
                "confirm",
                holder.confirm(token, at),
                tracker.confirm(token, at),
            ),
            12..=14 => ("expire", holder.expire(), tracker.expire()),
            _ => (
                "reacquire",
                holder.reacquire(token, at),
                tracker.reacquire(token, at),
            ),
        };
        assert_eq!(answer, expected, "{name} in {:?}", before.state());
        if answer != Ok(Recording::Recorded) {
            assert_eq!(*holder, before, "{name}: nothing may change");
        }
        self.seen.insert(format!("{name}:{answer:?}"));
    }

    /// Owner zero does what a holder is for: renew when due, and take a fresh
    /// grant when the authority says the old one is gone.
    pub(super) fn hold(&mut self) {
        let owner = OwnerId::new(0);
        let cadence = Cadence::new(at(8), at(2), at(3)).unwrap();
        let Some((holder, _)) = self.holder else {
            if let Ok(lease) = self.grant(owner, false) {
                let pair = (
                    Holder::new(lease.token(), self.now, cadence),
                    Tracker::new(lease.token(), self.now, cadence),
                );
                self.holder = Some(pair);
            }
            return;
        };
        if holder.state() == HolderState::Fenced {
            self.holder = None;
            return;
        }
        if !holder.renewal_due(self.now) {
            return;
        }
        // A holder told its grant expired does not ask to renew it.
        let renewed = if holder.state() == HolderState::Expired {
            Err(Refusal::Expired)
        } else {
            self.renew(owner, holder.token())
        };
        let fresh = if renewed.is_err() {
            self.grant(owner, true).ok()
        } else {
            None
        };
        let now = self.now;
        let (holder, tracker) = self.holder.as_mut().unwrap();
        assert_eq!(holder.refresh(now), tracker.refresh(now));
        if let Ok(lease) = renewed {
            assert_eq!(holder.confirm(lease.token(), now), Ok(Recording::Recorded));
            let _ = tracker.confirm(lease.token(), now);
        } else {
            assert!(holder.expire().is_ok());
            let _ = tracker.expire();
            if let Some(lease) = fresh {
                // A fresh grant is always newer than anything the holder saw.
                assert_eq!(
                    holder.reacquire(lease.token(), now),
                    Ok(Recording::Recorded)
                );
                let _ = tracker.reacquire(lease.token(), now);
            }
        }
    }

    pub(super) fn check_holder(&self) {
        let Some((holder, tracker)) = &self.holder else {
            return;
        };
        assert_eq!(holder.state(), tracker.state);
        assert_eq!(holder.token(), tracker.token);
        assert_eq!(holder.warning_deadline(), tracker.warning_deadline());
        for now in [self.now, self.now.saturating_sub(at(3)), self.now + at(3)] {
            assert_eq!(holder.renewal_due(now), tracker.renewal_due(now));
            assert_eq!(holder.at_risk(now), tracker.at_risk(now));
        }
    }
}

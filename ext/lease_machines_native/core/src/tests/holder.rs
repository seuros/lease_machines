//! The holder's renewal tracker: every report in every state.
use super::*;

fn cadence() -> Cadence {
    Cadence::new(at(30), at(10), at(5)).unwrap()
}

fn held(sequence: u64) -> FencingToken {
    token(SCOPE, sequence)
}

/// A holder of token 5 driven into `state`.
fn holder_in(state: HolderState) -> Holder {
    let mut holder = Holder::new(held(5), at(0), cadence());
    match state {
        HolderState::Confirmed => {}
        HolderState::Unconfirmed => drop(holder.refresh(at(10))),
        HolderState::AtRisk => drop(holder.refresh(at(25))),
        HolderState::Expired => drop(holder.expire()),
        HolderState::Fenced => holder.fence(),
    }
    assert_eq!(holder.state(), state);
    holder
}

const STATES: [HolderState; 5] = [
    HolderState::Confirmed,
    HolderState::Unconfirmed,
    HolderState::AtRisk,
    HolderState::Expired,
    HolderState::Fenced,
];

type Answer = Result<Recording, Refusal>;

/// Every answer is checked whole, and every refusal and duplicate against a
/// clone taken before the call.
fn check(state: HolderState, expected: Answer, call: impl FnOnce(&mut Holder) -> Answer) {
    let mut holder = holder_in(state);
    let before = holder;
    assert_eq!(call(&mut holder), expected, "in {state:?}");
    if expected == Ok(Recording::Recorded) {
        assert_ne!(holder, before);
    } else {
        assert_eq!(holder, before, "in {state:?}");
    }
}

#[test]
fn cadence_rejects_renewals_outside_the_ttl() {
    assert_eq!(
        Cadence::new(Duration::ZERO, Duration::ZERO, Duration::ZERO),
        None
    );
    assert_eq!(Cadence::new(at(30), at(30), at(5)), None);
    assert_eq!(Cadence::new(at(30), at(10), at(31)), None);
    let cadence = cadence();
    assert_eq!(
        (cadence.ttl(), cadence.renew_after(), cadence.warn_before()),
        (at(30), at(10), at(5)),
    );
    assert_eq!(holder_in(HolderState::Confirmed).cadence(), cadence);
}

#[test]
fn renewal_is_due_at_the_fraction_and_risk_at_the_warning_deadline() {
    let holder = Holder::new(held(1), at(100), cadence());
    assert_eq!(holder.state(), HolderState::Confirmed);
    assert_eq!(holder.token(), held(1));
    assert!(!holder.renewal_due(at(109)));
    assert!(holder.renewal_due(at(110)));
    assert_eq!(holder.warning_deadline(), Some(at(125)));
    assert!(!holder.at_risk(at(124)));
    assert!(holder.at_risk(at(125)));
}

#[test]
fn refresh_starts_one_attempt_and_reports_the_rest_as_outstanding() {
    let mut holder = Holder::new(held(1), at(0), cadence());
    assert_eq!(holder.refresh(at(10)), Ok(Refreshing::Started));
    assert_eq!(holder.state(), HolderState::Unconfirmed);
    assert!(holder.renewal_due(at(10)));
    assert!(holder.at_risk(at(10)));
    let before = holder;
    assert_eq!(holder.refresh(at(11)), Ok(Refreshing::Outstanding));
    assert_eq!(holder, before);
    // Past the warning deadline the outstanding attempt is at risk.
    assert_eq!(holder.refresh(at(25)), Ok(Refreshing::Outstanding));
    assert_eq!(holder.state(), HolderState::AtRisk);
    let before = holder;
    assert_eq!(holder.refresh(at(26)), Ok(Refreshing::Outstanding));
    assert_eq!(holder, before);

    let mut late = Holder::new(held(1), at(0), cadence());
    assert_eq!(late.refresh(at(26)), Ok(Refreshing::Started));
    assert_eq!(late.state(), HolderState::AtRisk);

    let mut expired = holder_in(HolderState::Expired);
    let before = expired;
    assert_eq!(expired.refresh(at(1)), Ok(Refreshing::Outstanding));
    assert_eq!(expired, before);
    assert!(expired.renewal_due(at(1)));

    let mut fenced = holder_in(HolderState::Fenced);
    refuses(&mut fenced, Refusal::Fenced, |h| h.refresh(at(1)));
}

#[test]
fn confirm_answers_in_every_state_for_every_kind_of_token() {
    use HolderState::{AtRisk, Confirmed, Expired, Fenced, Unconfirmed};
    let (older, same, newer) = (held(4), held(5), held(6));
    for state in [Unconfirmed, AtRisk] {
        check(state, Ok(Recording::Recorded), |h| h.confirm(same, at(27)));
        check(state, Ok(Recording::Recorded), |h| h.confirm(newer, at(27)));
        check(state, Err(Refusal::StaleToken), |h| {
            h.confirm(older, at(27))
        });
    }
    // Already confirmed under that token: honoured, and nothing rescheduled.
    check(Confirmed, Ok(Recording::Duplicate), |h| {
        h.confirm(same, at(27))
    });
    check(Confirmed, Err(Refusal::OutOfPhase), |h| {
        h.confirm(newer, at(27))
    });
    check(Confirmed, Err(Refusal::StaleToken), |h| {
        h.confirm(older, at(27))
    });
    check(Expired, Err(Refusal::Expired), |h| h.confirm(same, at(27)));
    check(Expired, Err(Refusal::Expired), |h| h.confirm(newer, at(27)));
    check(Expired, Err(Refusal::StaleToken), |h| {
        h.confirm(older, at(27))
    });
    for token in [older, same, newer, token(OTHER, 5)] {
        check(Fenced, Err(Refusal::Fenced), |h| h.confirm(token, at(27)));
    }
    for state in [Confirmed, Unconfirmed, AtRisk, Expired] {
        check(state, Err(Refusal::Unknown), |h| {
            h.confirm(token(OTHER, 6), at(27))
        });
    }
}

#[test]
fn reacquire_answers_in_every_state_for_every_kind_of_token() {
    use HolderState::{AtRisk, Confirmed, Expired, Fenced, Unconfirmed};
    let (older, same, newer) = (held(4), held(5), held(6));
    check(Expired, Ok(Recording::Recorded), |h| {
        h.reacquire(newer, at(40))
    });
    // The expired grant's own token restores nothing.
    check(Expired, Err(Refusal::StaleToken), |h| {
        h.reacquire(same, at(40))
    });
    check(Expired, Err(Refusal::StaleToken), |h| {
        h.reacquire(older, at(40))
    });
    // Confirmed under that very token: the report already holds.
    check(Confirmed, Ok(Recording::Duplicate), |h| {
        h.reacquire(same, at(40))
    });
    check(Confirmed, Err(Refusal::OutOfPhase), |h| {
        h.reacquire(newer, at(40))
    });
    check(Confirmed, Err(Refusal::StaleToken), |h| {
        h.reacquire(older, at(40))
    });
    for state in [Unconfirmed, AtRisk] {
        check(state, Err(Refusal::OutOfPhase), |h| {
            h.reacquire(same, at(40))
        });
        check(state, Err(Refusal::OutOfPhase), |h| {
            h.reacquire(newer, at(40))
        });
        check(state, Err(Refusal::StaleToken), |h| {
            h.reacquire(older, at(40))
        });
    }
    for token in [older, same, newer, token(OTHER, 6)] {
        check(Fenced, Err(Refusal::Fenced), |h| h.reacquire(token, at(40)));
    }
    for state in [Confirmed, Unconfirmed, AtRisk, Expired] {
        check(state, Err(Refusal::Unknown), |h| {
            h.reacquire(token(OTHER, 6), at(40))
        });
    }
}

#[test]
fn expire_and_fence_in_every_state() {
    use HolderState::{AtRisk, Confirmed, Expired, Fenced, Unconfirmed};
    for state in [Confirmed, Unconfirmed, AtRisk] {
        check(state, Ok(Recording::Recorded), Holder::expire);
    }
    check(Expired, Ok(Recording::Duplicate), Holder::expire);
    check(Fenced, Err(Refusal::Fenced), Holder::expire);
    for state in STATES {
        let mut holder = holder_in(state);
        holder.fence();
        assert_eq!(holder.state(), Fenced);
        let before = holder;
        holder.fence();
        assert_eq!(holder, before);
        assert!(!holder.renewal_due(at(3)));
        assert_eq!(holder.warning_deadline(), None);
        assert!(holder.at_risk(at(3)));
    }
}

#[test]
fn confirmation_reschedules_and_a_duplicate_does_not() {
    let mut holder = Holder::new(held(2), at(0), cadence());
    holder.refresh(at(10)).unwrap();
    assert_eq!(holder.confirm(held(2), at(11)), Ok(Recording::Recorded));
    assert_eq!(holder.state(), HolderState::Confirmed);
    assert_eq!(holder.warning_deadline(), Some(at(36)));
    // The reply delivered twice: honoured, and the schedule stays at 11.
    repeats(&mut holder, Recording::Duplicate, |h| {
        h.confirm(held(2), at(20))
    });
    assert_eq!(holder.warning_deadline(), Some(at(36)));
    assert!(!holder.renewal_due(at(20)));
    holder.refresh(at(21)).unwrap();
    assert_eq!(holder.confirm(held(3), at(22)), Ok(Recording::Recorded));
    assert_eq!(holder.token(), held(3));
    // A late reply for the old token.
    refuses(&mut holder, Refusal::StaleToken, |h| {
        h.confirm(held(2), at(23))
    });
}

#[test]
fn expiry_requires_a_newer_grant() {
    let mut holder = Holder::new(held(1), at(0), cadence());
    assert_eq!(holder.expire(), Ok(Recording::Recorded));
    assert_eq!(holder.state(), HolderState::Expired);
    assert!(holder.renewal_due(at(1)));
    assert_eq!(holder.reacquire(held(2), at(2)), Ok(Recording::Recorded));
    assert_eq!(holder.state(), HolderState::Confirmed);
    assert_eq!(holder.token(), held(2));
    assert_eq!(holder.warning_deadline(), Some(at(27)));
    repeats(&mut holder, Recording::Duplicate, |h| {
        h.reacquire(held(2), at(9))
    });
    assert_eq!(holder.warning_deadline(), Some(at(27)));
}

#[test]
fn deadlines_saturate_instead_of_overflowing() {
    let holder = Holder::new(held(1), Duration::MAX, cadence());
    assert_eq!(holder.warning_deadline(), Some(Duration::MAX));
    assert!(holder.renewal_due(Duration::MAX));
}

//! The observed clock: checked on every call, recorded only by honoured ones.
use periscope_machines::SEQUENCE_MAX;

use super::*;

/// The defect this conversion fixed: every refusal used to record its `now`
/// before anything else was checked, so a refused request moved the watermark.
#[test]
fn a_refusal_never_moves_the_watermark() {
    let mut authority = LeaseAuthority::with_scope(SCOPE);
    let old = authority.acquire(FIRST, at(0), at(10)).unwrap().token();
    let live = authority.reacquire(FIRST, at(0), at(10)).unwrap().token();
    assert_eq!(authority.checkpoint().observed(), at(0));
    refuses(&mut authority, Refusal::InvalidTtl, |a| {
        a.acquire(SECOND, at(1), Duration::ZERO)
    });
    refuses(&mut authority, Refusal::Occupied, |a| {
        a.acquire(SECOND, at(2), at(5))
    });
    refuses(&mut authority, Refusal::Held, |a| {
        a.acquire(FIRST, at(3), at(5))
    });
    refuses(&mut authority, Refusal::WrongOwner, |a| {
        a.renew(SECOND, live, at(4), at(5))
    });
    refuses(&mut authority, Refusal::StaleToken, |a| {
        a.release(FIRST, old, at(5))
    });
    refuses(&mut authority, Refusal::InvalidTtl, |a| {
        a.renew(FIRST, live, at(6), Duration::ZERO)
    });
    refuses(&mut authority, Refusal::Unknown, |a| {
        a.renew(FIRST, token(OTHER, 1), at(7), at(5))
    });
    refuses(&mut authority, Refusal::Expired, |a| {
        a.renew(FIRST, live, at(20), at(5))
    });
    refuses(&mut authority, Refusal::Expired, |a| {
        a.release(FIRST, live, at(21))
    });
    assert_eq!(authority.checkpoint().observed(), at(0));
    // Nothing above was recorded, so none of it makes an earlier instant a
    // regression.
    assert_eq!(authority.release(FIRST, live, at(1)).unwrap().token(), live);
    assert_eq!(authority.checkpoint().observed(), at(1));
    refuses(&mut authority, Refusal::NoLease, |a| {
        a.release(FIRST, live, at(30))
    });
    assert_eq!(authority.checkpoint().observed(), at(1));
}

#[test]
fn an_exhausted_refusal_does_not_move_the_watermark_either() {
    let spent = Checkpoint::from_parts(SCOPE, SEQUENCE_MAX, None, at(3)).unwrap();
    let mut authority = LeaseAuthority::restored(SCOPE, spent, ClockPolicy::Reject).unwrap();
    refuses(&mut authority, Refusal::Exhausted, |a| {
        a.acquire(FIRST, at(9), at(1))
    });
    assert_eq!(authority.checkpoint().observed(), at(3));
}

#[test]
fn every_honoured_call_records_its_instant() {
    let mut authority = LeaseAuthority::with_scope(SCOPE);
    let lease = authority.acquire(FIRST, at(1), at(10)).unwrap();
    assert_eq!(authority.checkpoint().observed(), at(1));
    authority
        .renew(FIRST, lease.token(), at(2), at(10))
        .unwrap();
    assert_eq!(authority.checkpoint().observed(), at(2));
    let rotated = authority.reacquire(FIRST, at(3), at(10)).unwrap();
    assert_eq!(authority.checkpoint().observed(), at(3));
    authority.release(FIRST, rotated.token(), at(4)).unwrap();
    assert_eq!(authority.checkpoint().observed(), at(4));
}

#[test]
fn rejecting_policy_refuses_every_regressing_call() {
    let mut authority = LeaseAuthority::with_scope(SCOPE);
    assert_eq!(authority.clock_policy(), ClockPolicy::Reject);
    let lease = authority.acquire(FIRST, at(2), at(5)).unwrap();
    refuses(&mut authority, Refusal::ClockRegressed, |a| {
        a.acquire(SECOND, at(1), at(5))
    });
    refuses(&mut authority, Refusal::ClockRegressed, |a| {
        a.reacquire(FIRST, at(1), at(5))
    });
    refuses(&mut authority, Refusal::ClockRegressed, |a| {
        a.renew(FIRST, lease.token(), at(1), at(5))
    });
    refuses(&mut authority, Refusal::ClockRegressed, |a| {
        a.release(FIRST, lease.token(), at(1))
    });
    // The same instant again is not a regression.
    assert!(authority.renew(FIRST, lease.token(), at(2), at(5)).is_ok());
}

#[test]
fn clamping_policy_evaluates_at_the_watermark() {
    let mut authority = LeaseAuthority::with_scope(SCOPE).with_clock_policy(ClockPolicy::Clamp);
    assert_eq!(authority.clock_policy(), ClockPolicy::Clamp);
    let lease = authority.acquire(FIRST, at(10), at(5)).unwrap();
    // Evaluated at 10, not 3: the deadline is 15 and the watermark stays.
    let renewed = authority.renew(FIRST, lease.token(), at(3), at(5)).unwrap();
    assert_eq!(renewed.expires_at(), at(15));
    assert_eq!(authority.checkpoint().observed(), at(10));
    // A refusal at a later instant is not recorded under this policy either.
    refuses(&mut authority, Refusal::Occupied, |a| {
        a.acquire(SECOND, at(14), at(5))
    });
    // A clamped call refused for another reason changes nothing.
    refuses(&mut authority, Refusal::Occupied, |a| {
        a.acquire(SECOND, at(1), at(5))
    });
    refuses(&mut authority, Refusal::InvalidTtl, |a| {
        a.renew(FIRST, lease.token(), at(1), Duration::ZERO)
    });
    assert_eq!(authority.release(FIRST, lease.token(), at(2)), Ok(renewed));
    assert_eq!(authority.checkpoint().observed(), at(10));
    let late = authority.acquire(SECOND, at(1), at(5)).unwrap();
    assert_eq!(late.expires_at(), at(15));
}

#[test]
fn clamping_cannot_revive_a_lease_another_owner_replaced() {
    let mut authority = LeaseAuthority::with_scope(SCOPE).with_clock_policy(ClockPolicy::Clamp);
    let first = authority.acquire(FIRST, at(0), at(5)).unwrap();
    let second = authority.acquire(SECOND, at(5), at(5)).unwrap();
    // The clock steps back to when the first grant was live.
    refuses(&mut authority, Refusal::WrongOwner, |a| {
        a.renew(FIRST, first.token(), at(1), at(5))
    });
    refuses(&mut authority, Refusal::Occupied, |a| {
        a.acquire(FIRST, at(1), at(5))
    });
    assert_eq!(authority.checkpoint().lease(), Some(second));
}

/// What "a refusal changes nothing" costs, stated as a test: a refused
/// `Expired` is not remembered, so the grant is still the owner's to renew if
/// a later call brings an instant at which it is live. Exclusion never rested
/// on that memory: nobody else holds a grant until an honoured `acquire`
/// records the instant that ended this one.
#[test]
fn a_refused_expiry_is_not_remembered() {
    let mut authority = LeaseAuthority::with_scope(SCOPE);
    let lease = authority.acquire(FIRST, at(0), at(13)).unwrap();
    refuses(&mut authority, Refusal::Expired, |a| {
        a.renew(FIRST, lease.token(), at(14), at(5))
    });
    assert!(authority.renew(FIRST, lease.token(), at(12), at(5)).is_ok());
    // An honoured acquisition is remembered, and ends the matter.
    let next = authority.acquire(SECOND, at(17), at(5)).unwrap();
    refuses(&mut authority, Refusal::ClockRegressed, |a| {
        a.renew(FIRST, lease.token(), at(12), at(5))
    });
    refuses(&mut authority, Refusal::WrongOwner, |a| {
        a.renew(FIRST, lease.token(), at(17), at(5))
    });
    assert_eq!(authority.checkpoint().lease(), Some(next));
}

/// `Expired` is an answer about the instant supplied, under either policy:
/// the same renewal at an earlier instant, not behind the latest honoured
/// one, is honoured. Exclusion holds all the same. One grant slot; another
/// owner enters only through an honoured `acquire`, which records its instant
/// and takes a newer token.
#[test]
fn expired_is_an_answer_about_the_instant_supplied() {
    for policy in [ClockPolicy::Reject, ClockPolicy::Clamp] {
        let mut authority = LeaseAuthority::with_scope(SCOPE).with_clock_policy(policy);
        let first = authority.acquire(FIRST, at(0), at(10)).unwrap().token();
        assert_eq!(first, token(SCOPE, 1));
        refuses(&mut authority, Refusal::Expired, |a| {
            a.renew(FIRST, first, at(15), at(10))
        });
        // The refusal recorded nothing: 5s is not behind the watermark (0s),
        // and at 5s the grant is live.
        assert_eq!(
            authority.renew(FIRST, first, at(5), at(10)),
            Ok(Lease::new(FIRST, first, at(15))),
            "{policy:?}"
        );
        assert_eq!(authority.checkpoint().observed(), at(5));
        // Nobody got in between: the slot was the first owner's throughout.
        refuses(&mut authority, Refusal::Occupied, |a| {
            a.acquire(SECOND, at(12), at(10))
        });
        let second = authority.acquire(SECOND, at(15), at(10)).unwrap();
        assert_eq!(second.token(), token(SCOPE, 2));
        // That acquisition was honoured, so it is remembered.
        refuses(&mut authority, Refusal::WrongOwner, |a| {
            a.renew(FIRST, first, at(16), at(10))
        });
        assert_eq!(authority.checkpoint().lease(), Some(second));
    }
}

/// The holder's side of the same story: a holder told `Expired` that
/// recorded it with `expire` refuses a confirmation that arrives late, even
/// though the authority honoured the renewal behind it.
#[test]
fn a_holder_that_recorded_expiry_refuses_the_late_confirmation() {
    let mut authority = LeaseAuthority::with_scope(SCOPE);
    let first = authority.acquire(FIRST, at(0), at(10)).unwrap().token();
    let cadence = Cadence::new(at(10), at(3), at(2)).unwrap();
    let mut holder = Holder::new(first, at(0), cadence);
    assert_eq!(holder.refresh(at(3)), Ok(Refreshing::Started));
    refuses(&mut authority, Refusal::Expired, |a| {
        a.renew(FIRST, first, at(15), at(10))
    });
    assert_eq!(holder.expire(), Ok(Recording::Recorded));
    let renewed = authority.renew(FIRST, first, at(5), at(10)).unwrap();
    refuses(&mut holder, Refusal::Expired, |h| {
        h.confirm(renewed.token(), at(6))
    });
    // Only a strictly newer token brings it back.
    refuses(&mut holder, Refusal::StaleToken, |h| {
        h.reacquire(renewed.token(), at(6))
    });
}

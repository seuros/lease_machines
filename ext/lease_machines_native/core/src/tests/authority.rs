//! Grants, renewals and releases, and every way each can be refused.
use periscope_machines::SEQUENCE_MAX;

use super::*;

#[test]
fn live_lease_excludes_competitors_and_expires_at_the_boundary() {
    let mut authority = LeaseAuthority::with_scope(SCOPE);
    let first = authority.acquire(FIRST, at(0), at(5)).unwrap();
    assert_eq!(first, Lease::new(FIRST, token(SCOPE, 1), at(5)));
    assert_eq!(first.owner().value(), 1);
    refuses(&mut authority, Refusal::Occupied, |a| {
        a.acquire(SECOND, at(4), at(5))
    });
    let second = authority.acquire(SECOND, at(5), at(5)).unwrap();
    assert!(second.token().is_newer_than(first.token()));
    assert_eq!(authority.issued(), 2);
}

#[test]
fn scope_zero_tokens_read_one_two_three() {
    let mut authority = LeaseAuthority::new();
    assert_eq!(authority.scope(), 0);
    assert_eq!(LeaseAuthority::default(), authority);
    for expected in 1..=3 {
        let lease = authority.reacquire(FIRST, at(0), at(5)).unwrap();
        assert_eq!(lease.token().raw(), expected);
    }
}

#[test]
fn renewal_retains_token_and_cannot_shorten_or_resurrect() {
    let mut authority = LeaseAuthority::with_scope(SCOPE);
    let lease = authority.acquire(FIRST, at(0), at(5)).unwrap();
    assert_eq!(
        authority.renew(FIRST, lease.token(), at(1), at(1)),
        Ok(lease)
    );
    let extended = authority.renew(FIRST, lease.token(), at(4), at(5)).unwrap();
    assert_eq!(extended, Lease::new(FIRST, lease.token(), at(9)));
    // A replayed renewal is honoured and loses no time.
    assert_eq!(
        authority.renew(FIRST, lease.token(), at(4), at(5)),
        Ok(extended)
    );
    refuses(&mut authority, Refusal::Expired, |a| {
        a.renew(FIRST, lease.token(), at(9), at(5))
    });
}

#[test]
fn release_answers_the_grant_and_keeps_the_token_count() {
    let mut authority = LeaseAuthority::with_scope(SCOPE);
    let first = authority.acquire(FIRST, at(0), at(5)).unwrap();
    assert_eq!(authority.release(FIRST, first.token(), at(1)), Ok(first));
    assert_eq!(authority.checkpoint().lease(), None);
    assert_eq!(authority.issued(), 1);
    // No record of who released what: a repeat cannot be proved.
    refuses(&mut authority, Refusal::NoLease, |a| {
        a.release(FIRST, first.token(), at(1))
    });
    refuses(&mut authority, Refusal::NoLease, |a| {
        a.renew(FIRST, first.token(), at(1), at(5))
    });
    let next = authority.acquire(FIRST, at(1), at(5)).unwrap();
    assert!(next.token().is_newer_than(first.token()));
    refuses(&mut authority, Refusal::StaleToken, |a| {
        a.release(FIRST, first.token(), at(2))
    });
}

#[test]
fn owner_and_token_are_both_required() {
    let mut authority = LeaseAuthority::with_scope(SCOPE);
    let old = authority.acquire(FIRST, at(0), at(5)).unwrap().token();
    let lease = authority.reacquire(FIRST, at(0), at(5)).unwrap();
    let live = lease.token();
    refuses(&mut authority, Refusal::WrongOwner, |a| {
        a.renew(SECOND, live, at(1), at(5))
    });
    refuses(&mut authority, Refusal::WrongOwner, |a| {
        a.release(SECOND, live, at(1))
    });
    refuses(&mut authority, Refusal::StaleToken, |a| {
        a.renew(FIRST, old, at(1), at(5))
    });
    refuses(&mut authority, Refusal::StaleToken, |a| {
        a.release(FIRST, old, at(1))
    });
    // The wrong owner is reported before the stale token.
    refuses(&mut authority, Refusal::WrongOwner, |a| {
        a.release(SECOND, old, at(1))
    });
    assert_eq!(authority.checkpoint().lease(), Some(lease));
}

/// An expired grant nobody replaced is still the grant in force: the wrong
/// owner and a rotated-out token are refused for what they are, not as
/// `Expired`, which only its rightful holder hears.
#[test]
fn credentials_are_judged_before_expiry() {
    for policy in [ClockPolicy::Reject, ClockPolicy::Clamp] {
        let mut authority = LeaseAuthority::with_scope(SCOPE).with_clock_policy(policy);
        let old = authority.acquire(FIRST, at(0), at(10)).unwrap().token();
        let lease = authority.reacquire(FIRST, at(0), at(10)).unwrap();
        let live = lease.token();
        let late = at(15);
        for (owner, token, refusal) in [
            (SECOND, live, Refusal::WrongOwner),
            (SECOND, old, Refusal::WrongOwner),
            (FIRST, old, Refusal::StaleToken),
            (FIRST, live, Refusal::Expired),
        ] {
            refuses(&mut authority, refusal, |a| {
                a.renew(owner, token, late, at(10))
            });
            refuses(&mut authority, refusal, |a| a.release(owner, token, late));
            refuses(&mut authority, refusal, |a| a.check(owner, token, late));
        }
        assert_eq!(authority.checkpoint().lease(), Some(lease), "{policy:?}");
    }
}

#[test]
fn tokens_never_issued_are_unknown_before_any_lookup() {
    let mut authority = LeaseAuthority::with_scope(SCOPE);
    // Nothing issued, no lease: still Unknown, not NoLease.
    refuses(&mut authority, Refusal::Unknown, |a| {
        a.renew(FIRST, token(SCOPE, 1), at(0), at(5))
    });
    let live = authority.acquire(FIRST, at(0), at(5)).unwrap().token();
    let unissued = token(SCOPE, 2);
    let foreign = token(OTHER, 1);
    let zero = token(0, 1);
    for forged in [unissued, foreign, zero] {
        refuses(&mut authority, Refusal::Unknown, |a| {
            a.renew(FIRST, forged, at(1), at(5))
        });
        refuses(&mut authority, Refusal::Unknown, |a| {
            a.release(FIRST, forged, at(1))
        });
        // Unknown comes first: before the clock, the owner and the ttl.
        refuses(&mut authority, Refusal::Unknown, |a| {
            a.renew(SECOND, forged, at(0), Duration::ZERO)
        });
    }
    assert_eq!(
        authority.renew(FIRST, live, at(1), at(5)).unwrap().token(),
        live
    );
}

#[test]
fn the_owner_of_a_live_grant_must_renew_or_rotate() {
    let mut authority = LeaseAuthority::with_scope(SCOPE);
    let first = authority.acquire(FIRST, at(0), at(5)).unwrap();
    // Not an honoured repeat: the same request again proves nothing, and the
    // live token must not be handed out twice.
    refuses(&mut authority, Refusal::Held, |a| {
        a.acquire(FIRST, at(0), at(5))
    });
    refuses(&mut authority, Refusal::Occupied, |a| {
        a.reacquire(SECOND, at(1), at(5))
    });
    let rotated = authority.reacquire(FIRST, at(1), at(5)).unwrap();
    assert!(rotated.token().is_newer_than(first.token()));
    assert_eq!(rotated.expires_at(), at(6));
    refuses(&mut authority, Refusal::StaleToken, |a| {
        a.renew(FIRST, first.token(), at(2), at(5))
    });
    refuses(&mut authority, Refusal::Held, |a| {
        a.acquire(FIRST, at(2), at(5))
    });
    let after_expiry = authority.reacquire(SECOND, at(6), at(5)).unwrap();
    assert!(after_expiry.token().is_newer_than(rotated.token()));
    // Once expired, the old owner acquires like anyone else.
    let again = authority.acquire(FIRST, at(11), at(5)).unwrap();
    assert!(again.token().is_newer_than(after_expiry.token()));
}

#[test]
fn invalid_ttls_are_refused_and_consume_no_token() {
    let mut authority = LeaseAuthority::with_scope(SCOPE);
    refuses(&mut authority, Refusal::InvalidTtl, |a| {
        a.acquire(FIRST, at(1), Duration::ZERO)
    });
    refuses(&mut authority, Refusal::InvalidTtl, |a| {
        a.acquire(FIRST, Duration::MAX, at(1))
    });
    refuses(&mut authority, Refusal::InvalidTtl, |a| {
        a.reacquire(FIRST, at(1), Duration::ZERO)
    });
    assert_eq!(authority.issued(), 0);
    let lease = authority.acquire(FIRST, at(0), at(10)).unwrap();
    assert_eq!(lease.token(), token(SCOPE, 1));
    refuses(&mut authority, Refusal::InvalidTtl, |a| {
        a.renew(FIRST, lease.token(), at(2), Duration::ZERO)
    });
    refuses(&mut authority, Refusal::InvalidTtl, |a| {
        a.renew(FIRST, lease.token(), at(3), Duration::MAX)
    });
}

#[test]
fn an_exhausted_scope_fails_closed() {
    let spent = Checkpoint::from_parts(SCOPE, SEQUENCE_MAX - 1, None, at(0)).unwrap();
    let mut authority = LeaseAuthority::restored(SCOPE, spent, ClockPolicy::Reject).unwrap();
    let last = authority.acquire(FIRST, at(0), at(1)).unwrap();
    assert_eq!(last.token(), token(SCOPE, SEQUENCE_MAX));
    refuses(&mut authority, Refusal::Exhausted, |a| {
        a.acquire(SECOND, at(1), at(1))
    });
    refuses(&mut authority, Refusal::Exhausted, |a| {
        a.reacquire(FIRST, at(0), at(1))
    });
    // The last grant still renews and releases.
    assert!(authority.renew(FIRST, last.token(), at(0), at(5)).is_ok());
    assert!(authority.release(FIRST, last.token(), at(1)).is_ok());
    refuses(&mut authority, Refusal::Exhausted, |a| {
        a.acquire(FIRST, at(1), at(1))
    });
}

#[test]
fn refusals_say_why_in_words() {
    use std::string::ToString;
    let all = [
        Refusal::Occupied,
        Refusal::Held,
        Refusal::NoLease,
        Refusal::WrongOwner,
        Refusal::StaleToken,
        Refusal::Unknown,
        Refusal::Expired,
        Refusal::ClockRegressed,
        Refusal::InvalidTtl,
        Refusal::Exhausted,
        Refusal::Fenced,
        Refusal::OutOfPhase,
    ];
    let mut seen = std::collections::BTreeSet::new();
    for refusal in all {
        let error: &dyn core::error::Error = &refusal;
        assert!(
            seen.insert(error.to_string()),
            "{refusal:?} shares its text"
        );
    }
}

#[test]
fn incumbent_is_the_grant_an_acquire_would_meet() {
    let mut authority = LeaseAuthority::with_scope(SCOPE);
    assert_eq!(authority.incumbent(at(0), at(5)), Ok(None));
    let first = authority.acquire(FIRST, at(1), at(5)).unwrap();
    // Live for its owner (`Held`) and for anyone else (`Occupied`) alike.
    assert_eq!(authority.incumbent(at(1), at(5)), Ok(Some(first)));
    assert_eq!(authority.incumbent(at(5), at(5)), Ok(Some(first)));
    // Expired exactly at its deadline, as `acquire` sees it.
    assert_eq!(authority.incumbent(at(6), at(5)), Ok(None));
    // Checked in `acquire`'s order: the clock, then the ttl, then the grant.
    assert_eq!(
        authority.incumbent(at(0), at(5)),
        Err(Refusal::ClockRegressed)
    );
    assert_eq!(authority.incumbent(at(2), at(0)), Err(Refusal::InvalidTtl));
    let overflow = authority.incumbent(at(2), Duration::MAX);
    assert_eq!(overflow, Err(Refusal::InvalidTtl));
    // A query: nothing recorded, not even the instant.
    let before = authority.clone();
    let _ = authority.incumbent(at(9), at(5));
    assert_eq!(authority, before);
    assert_eq!(
        authority.acquire(SECOND, at(2), at(5)),
        Err(Refusal::Occupied)
    );
    assert_eq!(authority.release(FIRST, first.token(), at(2)), Ok(first));
    assert_eq!(authority.incumbent(at(2), at(5)), Ok(None));
}

#[test]
fn incumbent_under_clamp_is_evaluated_at_the_watermark() {
    let mut authority = LeaseAuthority::with_scope(SCOPE).with_clock_policy(ClockPolicy::Clamp);
    let first = authority.acquire(FIRST, at(4), at(5)).unwrap();
    // An earlier `now` is read as the watermark, where the grant is live.
    assert_eq!(authority.incumbent(at(0), at(5)), Ok(Some(first)));
    // The ttl overflows from the watermark, not from the earlier `now`.
    let overflow = Duration::MAX.checked_sub(at(2)).unwrap();
    assert_eq!(
        authority.incumbent(at(0), overflow),
        Err(Refusal::InvalidTtl)
    );
    assert_eq!(
        authority.acquire(FIRST, at(0), overflow),
        Err(Refusal::InvalidTtl)
    );
}

#[test]
fn check_is_a_renewal_without_the_ttl_or_the_record() {
    let mut authority = LeaseAuthority::with_scope(SCOPE);
    let old = authority.acquire(FIRST, at(1), at(5)).unwrap().token();
    let lease = authority.reacquire(FIRST, at(1), at(5)).unwrap();
    let live = lease.token();
    assert_eq!(authority.check(FIRST, live, at(1)), Ok(lease));
    assert_eq!(authority.check(FIRST, live, at(5)), Ok(lease));
    assert_eq!(authority.check(FIRST, live, at(6)), Err(Refusal::Expired));
    assert_eq!(
        authority.check(FIRST, token(OTHER, 2), at(0)),
        Err(Refusal::Unknown)
    );
    assert_eq!(
        authority.check(FIRST, token(SCOPE, 3), at(2)),
        Err(Refusal::Unknown)
    );
    assert_eq!(
        authority.check(SECOND, old, at(0)),
        Err(Refusal::ClockRegressed)
    );
    assert_eq!(
        authority.check(SECOND, live, at(2)),
        Err(Refusal::WrongOwner)
    );
    assert_eq!(authority.check(FIRST, old, at(2)), Err(Refusal::StaleToken));
    let before = authority.clone();
    let _ = authority.check(FIRST, live, at(4));
    assert_eq!(authority, before);
    assert_eq!(
        authority
            .renew(FIRST, live, at(2), at(5))
            .map(Lease::expires_at),
        Ok(at(7))
    );
    assert_eq!(
        authority.release(FIRST, live, at(3)).map(Lease::token),
        Ok(live)
    );
    assert_eq!(authority.check(FIRST, live, at(3)), Err(Refusal::NoLease));
}

#[test]
fn check_under_clamp_is_evaluated_at_the_watermark() {
    let mut authority = LeaseAuthority::with_scope(SCOPE).with_clock_policy(ClockPolicy::Clamp);
    let lease = authority.acquire(FIRST, at(4), at(5)).unwrap();
    assert_eq!(authority.check(FIRST, lease.token(), at(0)), Ok(lease));
    let next = authority.acquire(SECOND, at(9), at(5)).unwrap();
    assert_eq!(
        authority.check(FIRST, lease.token(), at(0)),
        Err(Refusal::WrongOwner)
    );
    assert_eq!(authority.check(SECOND, next.token(), at(0)), Ok(next));
}

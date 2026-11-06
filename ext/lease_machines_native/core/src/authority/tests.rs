#![allow(clippy::expect_used)] // Failed test assumptions should panic.

use super::*;

const FIRST: OwnerId = OwnerId::new(1);
const SECOND: OwnerId = OwnerId::new(2);

fn at(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

fn granted(decision: Decision) -> Lease {
    match decision {
        Decision::Granted(lease) => lease,
        other => panic!("expected a grant, got {other:?}"),
    }
}

fn renewed(decision: Decision) -> Lease {
    match decision {
        Decision::Renewed(lease) => lease,
        other => panic!("expected a renewal, got {other:?}"),
    }
}

#[test]
fn live_lease_excludes_competitors_and_expires_at_the_boundary() {
    let mut authority = LeaseAuthority::new();
    let first = granted(authority.acquire(FIRST, at(0), at(5)));
    assert_eq!(first.owner(), FIRST);
    assert_eq!(first.owner().value(), 1);
    assert_eq!(first.token().value(), 1);
    assert_eq!(
        authority.acquire(SECOND, at(4), at(5)),
        Decision::Rejected(Rejection::Occupied),
    );
    let second = granted(authority.acquire(SECOND, at(5), at(5)));
    assert!(second.token() > first.token());
}

#[test]
fn renewal_retains_token_and_cannot_shorten_or_resurrect() {
    let mut authority = LeaseAuthority::new();
    let lease = granted(authority.acquire(FIRST, at(0), at(5)));
    let unchanged = renewed(authority.renew(FIRST, lease.token(), at(1), at(1)));
    assert_eq!(unchanged, lease);
    let extended = renewed(authority.renew(FIRST, lease.token(), at(4), at(5)));
    assert_eq!(extended.token(), lease.token());
    assert_eq!(extended.expires_at(), at(9));
    assert_eq!(
        authority.renew(FIRST, lease.token(), at(9), at(5)),
        Decision::Rejected(Rejection::Expired),
    );
}

#[test]
fn release_preserves_token_history_and_reacquisition_uses_a_new_token() {
    let mut authority = LeaseAuthority::new();
    let first = granted(authority.acquire(FIRST, at(0), at(5)));
    assert_eq!(
        authority.release(FIRST, first.token(), at(1)),
        Decision::Released,
    );
    assert_eq!(authority.checkpoint().lease(), None);
    assert_eq!(authority.checkpoint().last_issued(), Some(first.token()));
    let next = granted(authority.acquire(FIRST, at(1), at(5)));
    assert!(next.token() > first.token());
    assert_eq!(
        authority.release(FIRST, first.token(), at(2)),
        Decision::Rejected(Rejection::StaleToken),
    );
}

#[test]
fn owner_and_token_are_both_required() {
    let mut authority = LeaseAuthority::new();
    let lease = granted(authority.acquire(FIRST, at(0), at(5)));
    let wrong = FencingToken::new(99).expect("positive token");
    for outcome in [
        authority.renew(SECOND, lease.token(), at(1), at(5)),
        authority.release(SECOND, lease.token(), at(1)),
    ] {
        assert_eq!(outcome, Decision::Rejected(Rejection::WrongOwner));
    }
    for outcome in [
        authority.renew(FIRST, wrong, at(1), at(5)),
        authority.release(FIRST, wrong, at(1)),
    ] {
        assert_eq!(outcome, Decision::Rejected(Rejection::StaleToken));
    }
    assert_eq!(authority.checkpoint().lease(), Some(lease));
}

#[test]
fn invalid_ttl_and_regressing_clocks_fail_closed() {
    let mut authority = LeaseAuthority::new();
    assert_eq!(
        authority.acquire(FIRST, at(1), Duration::ZERO),
        Decision::Rejected(Rejection::InvalidTtl),
    );
    assert_eq!(authority.checkpoint().observed(), at(1));
    assert_eq!(authority.checkpoint().last_issued(), None);
    let lease = granted(authority.acquire(FIRST, at(2), at(5)));
    let before = authority.checkpoint();
    for outcome in [
        authority.acquire(SECOND, at(1), at(5)),
        authority.renew(FIRST, lease.token(), at(1), at(5)),
        authority.release(FIRST, lease.token(), at(1)),
    ] {
        assert_eq!(outcome, Decision::Rejected(Rejection::ClockRegressed));
    }
    assert_eq!(authority.checkpoint(), before);
}

#[test]
fn overflow_and_token_exhaustion_do_not_issue_grants() {
    let mut authority = LeaseAuthority::new();
    assert_eq!(
        authority.acquire(FIRST, Duration::MAX, at(1)),
        Decision::Rejected(Rejection::InvalidTtl),
    );
    assert_eq!(authority.checkpoint().last_issued(), None);
    let mut exhausted = LeaseAuthority::restore(Checkpoint {
        last_issued: FencingToken::new(u64::MAX),
        lease: None,
        observed: Duration::ZERO,
    });
    assert_eq!(
        exhausted.acquire(FIRST, at(0), at(1)),
        Decision::Rejected(Rejection::TokenExhausted),
    );
    assert_eq!(
        exhausted.checkpoint().last_issued(),
        FencingToken::new(u64::MAX),
    );
}

#[test]
fn committed_checkpoint_preserves_live_grants_and_tokens() {
    let (lease, committed) = {
        let mut original = LeaseAuthority::new();
        let lease = granted(original.acquire(FIRST, at(10), at(5)));
        (lease, original.checkpoint())
    }; // Only the restored authority serves from this point.
    let mut restored = LeaseAuthority::restore(committed);
    assert_eq!(
        restored.acquire(SECOND, at(11), at(5)),
        Decision::Rejected(Rejection::Occupied),
    );
    assert_eq!(
        restored.acquire(SECOND, at(9), at(5)),
        Decision::Rejected(Rejection::ClockRegressed),
    );
    let next = granted(restored.acquire(SECOND, at(15), at(5)));
    assert!(next.token() > lease.token());
}

#[test]
fn absent_and_expired_leases_cannot_be_released_or_renewed() {
    let mut authority = LeaseAuthority::new();
    let token = FencingToken::new(1).expect("positive token");
    assert_eq!(
        authority.renew(FIRST, token, at(0), at(1)),
        Decision::Rejected(Rejection::NoLease),
    );
    assert_eq!(
        authority.release(FIRST, token, at(0)),
        Decision::Rejected(Rejection::NoLease),
    );
    let lease = granted(authority.acquire(FIRST, at(0), at(1)));
    assert_eq!(
        authority.release(FIRST, lease.token(), at(1)),
        Decision::Rejected(Rejection::Expired),
    );
}

#[test]
fn rejected_renewal_preserves_grant_but_advances_clock() {
    let mut authority = LeaseAuthority::new();
    let lease = granted(authority.acquire(FIRST, at(0), at(10)));
    assert_eq!(
        authority.renew(FIRST, lease.token(), at(2), Duration::ZERO),
        Decision::Rejected(Rejection::InvalidTtl),
    );
    assert_eq!(authority.checkpoint().lease(), Some(lease));
    assert_eq!(authority.checkpoint().observed(), at(2));
    assert_eq!(
        authority.renew(FIRST, lease.token(), at(3), Duration::MAX),
        Decision::Rejected(Rejection::InvalidTtl),
    );
    assert_eq!(authority.checkpoint().lease(), Some(lease));
}

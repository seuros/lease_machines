//! Committed state: what `from_parts` admits and what `restored` resumes.
use periscope_machines::SEQUENCE_MAX;

use super::*;

#[test]
fn restoring_resumes_the_grant_the_tokens_and_the_clock() {
    let mut original = LeaseAuthority::with_scope(SCOPE);
    let lease = original.acquire(FIRST, at(10), at(5)).unwrap();
    let committed = original.checkpoint();
    assert_eq!(committed.scope(), SCOPE);
    assert_eq!(committed.issued(), 1);
    assert_eq!(committed.lease(), Some(lease));
    assert_eq!(committed.observed(), at(10));
    let mut restored = LeaseAuthority::restored(SCOPE, committed, ClockPolicy::Reject).unwrap();
    assert_eq!(restored, original);
    refuses(&mut restored, Refusal::Occupied, |a| {
        a.acquire(SECOND, at(11), at(5))
    });
    refuses(&mut restored, Refusal::ClockRegressed, |a| {
        a.acquire(SECOND, at(9), at(5))
    });
    let next = restored.acquire(SECOND, at(15), at(5)).unwrap();
    assert_eq!(next.token(), token(SCOPE, 2));
    // The pre-restart token is still this authority's, and still stale.
    refuses(&mut restored, Refusal::WrongOwner, |a| {
        a.release(FIRST, lease.token(), at(15))
    });
}

#[test]
fn a_checkpoint_of_another_scope_is_refused() {
    let mut original = LeaseAuthority::with_scope(SCOPE);
    original.acquire(FIRST, at(0), at(5)).unwrap();
    for wrong in [0, OTHER, u16::MAX] {
        assert_eq!(
            LeaseAuthority::restored(wrong, original.checkpoint(), ClockPolicy::Reject),
            None,
        );
    }
    // A fresh authority's checkpoint names its scope too.
    let fresh = LeaseAuthority::with_scope(SCOPE).checkpoint();
    assert_eq!(fresh.issued(), 0);
    assert_eq!(
        LeaseAuthority::restored(0, fresh, ClockPolicy::Reject),
        None
    );
}

/// Reproduced before the conversion: `restore` silently went back to
/// `ClockPolicy::Reject`. The policy is now stated on every restore.
#[test]
fn restoring_states_the_clock_policy() {
    let mut original = LeaseAuthority::with_scope(SCOPE).with_clock_policy(ClockPolicy::Clamp);
    original.acquire(FIRST, at(10), at(5)).unwrap();
    let policy = original.clock_policy();
    let clamped = LeaseAuthority::restored(SCOPE, original.checkpoint(), policy).unwrap();
    assert_eq!(clamped.clock_policy(), ClockPolicy::Clamp);
    assert_eq!(clamped, original);
    let rejecting =
        LeaseAuthority::restored(SCOPE, original.checkpoint(), ClockPolicy::Reject).unwrap();
    assert_eq!(rejecting.clock_policy(), ClockPolicy::Reject);
    assert_ne!(rejecting, original);
}

#[test]
fn rows_rebuild_a_checkpoint_only_when_they_agree() {
    let live = token(SCOPE, 3);
    let lease = Lease::new(FIRST, live, at(9));
    assert_eq!(lease.owner(), FIRST);
    assert_eq!(lease.token(), live);
    assert_eq!(lease.expires_at(), at(9));
    let rows = |issued, lease| Checkpoint::from_parts(SCOPE, issued, lease, at(0));
    // Nothing issued, yet a lease.
    assert_eq!(rows(0, Some(lease)), None);
    // A token the history never issued.
    assert_eq!(rows(2, Some(lease)), None);
    // A superseded grant: token 4 exists, so token 3 is no longer the lease.
    assert_eq!(rows(4, Some(lease)), None);
    // A grant of another scope, whatever its number.
    assert_eq!(Checkpoint::from_parts(OTHER, 3, Some(lease), at(0)), None);
    assert_eq!(Checkpoint::from_parts(0, 3, Some(lease), at(0)), None);
    // A count no issuer reaches.
    assert_eq!(rows(SEQUENCE_MAX + 1, None), None);
    assert_eq!(rows(u64::MAX, None), None);
    assert!(rows(SEQUENCE_MAX, None).is_some());

    let committed = rows(3, Some(lease)).unwrap();
    assert_eq!(committed.lease(), Some(lease));
    let mut restored = LeaseAuthority::restored(SCOPE, committed, ClockPolicy::Reject).unwrap();
    refuses(&mut restored, Refusal::Occupied, |a| {
        a.acquire(SECOND, at(8), at(5))
    });
    let next = restored.acquire(SECOND, at(9), at(5)).unwrap();
    assert_eq!(next.token(), token(SCOPE, 4));

    let released = Checkpoint::from_parts(SCOPE, 3, None, at(1)).unwrap();
    assert_eq!(released.issued(), 3);
    assert_eq!(released.observed(), at(1));
    let mut vacant = LeaseAuthority::restored(SCOPE, released, ClockPolicy::Reject).unwrap();
    refuses(&mut vacant, Refusal::NoLease, |a| {
        a.release(FIRST, live, at(1))
    });
    assert_eq!(
        vacant.acquire(FIRST, at(1), at(5)).unwrap().token(),
        token(SCOPE, 4)
    );
}

#[test]
fn every_checkpoint_an_authority_takes_restores_to_itself() {
    let mut authority = LeaseAuthority::with_scope(SCOPE);
    let same = |authority: &LeaseAuthority| {
        let saved = authority.checkpoint();
        let rebuilt = Checkpoint::from_parts(
            saved.scope(),
            saved.issued(),
            saved.lease(),
            saved.observed(),
        );
        assert_eq!(rebuilt, Some(saved.clone()));
        let restored = LeaseAuthority::restored(SCOPE, saved, authority.clock_policy());
        assert_eq!(restored.as_ref(), Some(authority));
    };
    same(&authority);
    let lease = authority.acquire(FIRST, at(1), at(5)).unwrap();
    same(&authority);
    authority.renew(FIRST, lease.token(), at(2), at(9)).unwrap();
    same(&authority);
    let rotated = authority.reacquire(FIRST, at(3), at(5)).unwrap();
    same(&authority);
    authority.release(FIRST, rotated.token(), at(4)).unwrap();
    same(&authority);
}

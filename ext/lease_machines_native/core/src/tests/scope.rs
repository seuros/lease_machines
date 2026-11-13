//! Two authorities, their tokens, and the fences and holders they meet.
use super::*;

/// Reproduced before the conversion: two fresh authorities issued the same
/// tokens, and a fence accepted a token from either.
#[test]
fn two_authorities_issue_tokens_nobody_can_confuse() {
    let mut here = LeaseAuthority::with_scope(SCOPE);
    let mut there = LeaseAuthority::with_scope(OTHER);
    assert_eq!((here.scope(), there.scope()), (SCOPE, OTHER));
    let mine = here.acquire(FIRST, at(0), at(5)).unwrap();
    let theirs = there.acquire(FIRST, at(0), at(5)).unwrap();
    assert_ne!(mine.token(), theirs.token());
    assert_eq!(mine.token().scope(), SCOPE);
    assert_eq!(theirs.token().scope(), OTHER);

    // Same owner, same instant, same sequence number: still not this
    // authority's grant.
    refuses(&mut here, Refusal::Unknown, |a| {
        a.renew(FIRST, theirs.token(), at(1), at(5))
    });
    refuses(&mut here, Refusal::Unknown, |a| {
        a.release(FIRST, theirs.token(), at(1))
    });
    refuses(&mut there, Refusal::Unknown, |a| {
        a.release(FIRST, mine.token(), at(1))
    });

    let mut fence = Fence::with_scope(SCOPE);
    assert_eq!(fence.accept(mine.token()), Ok(Accepting::Advanced));
    refuses(&mut fence, Refusal::Unknown, |f| f.accept(theirs.token()));
    // However many grants the other authority makes, none outranks this one.
    for _ in 0..5 {
        let newest = there.reacquire(FIRST, at(1), at(5)).unwrap();
        refuses(&mut fence, Refusal::Unknown, |f| f.accept(newest.token()));
    }
    assert_eq!(fence.highest(), Some(mine.token()));
}

#[test]
fn two_authorities_left_in_one_scope_still_collide() {
    // The scope is the caller's to keep distinct; the core cannot.
    let mut one = LeaseAuthority::with_scope(SCOPE);
    let mut two = LeaseAuthority::with_scope(SCOPE);
    let a = one.acquire(FIRST, at(0), at(5)).unwrap();
    let b = two.acquire(SECOND, at(0), at(5)).unwrap();
    assert_eq!(a.token(), b.token());
}

/// Reproduced before the conversion: restoring an older checkpoint reissued
/// tokens. Nothing in a pure value can stop a rollback, so the test pins what
/// the contract says instead: the rolled-back authority is a second authority
/// in the same scope, and only a new scope keeps its tokens apart.
#[test]
fn a_rolled_back_checkpoint_is_a_second_authority() {
    let mut authority = LeaseAuthority::with_scope(SCOPE);
    let saved = authority.checkpoint();
    let first = authority.acquire(FIRST, at(0), at(5)).unwrap();
    let mut rolled = LeaseAuthority::restored(SCOPE, saved, ClockPolicy::Reject).unwrap();
    let again = rolled.acquire(SECOND, at(0), at(5)).unwrap();
    assert_eq!(again.token(), first.token());
    // The other way to restart: a scope no earlier incarnation used.
    let mut reborn = LeaseAuthority::with_scope(OTHER);
    let fresh = reborn.acquire(SECOND, at(0), at(5)).unwrap();
    assert_ne!(fresh.token(), first.token());
    refuses(&mut reborn, Refusal::Unknown, |a| {
        a.renew(FIRST, first.token(), at(1), at(5))
    });
    // The fence must be rebuilt for the new scope, and then the old
    // incarnation's tokens are the foreign ones.
    let mut fence = Fence::with_scope(OTHER);
    assert_eq!(fence.accept(fresh.token()), Ok(Accepting::Advanced));
    refuses(&mut fence, Refusal::Unknown, |f| f.accept(first.token()));
}

#[test]
fn a_restored_authority_still_owns_what_its_predecessor_issued() {
    let mut original = LeaseAuthority::with_scope(SCOPE);
    let old = original.acquire(FIRST, at(0), at(5)).unwrap();
    let live = original.reacquire(FIRST, at(1), at(5)).unwrap();
    let mut restored =
        LeaseAuthority::restored(SCOPE, original.checkpoint(), ClockPolicy::Reject).unwrap();
    refuses(&mut restored, Refusal::StaleToken, |a| {
        a.renew(FIRST, old.token(), at(2), at(5))
    });
    refuses(&mut restored, Refusal::Unknown, |a| {
        a.renew(FIRST, token(SCOPE, 3), at(2), at(5))
    });
    assert!(restored.renew(FIRST, live.token(), at(2), at(5)).is_ok());
    let next = restored.reacquire(FIRST, at(3), at(5)).unwrap();
    assert_eq!(next.token(), token(SCOPE, 3));
}

#[test]
fn a_holder_follows_the_tokens_of_one_scope() {
    let cadence = Cadence::new(at(30), at(10), at(5)).unwrap();
    let mut holder = Holder::new(token(SCOPE, 2), at(0), cadence);
    holder.refresh(at(10)).unwrap();
    refuses(&mut holder, Refusal::Unknown, |h| {
        h.confirm(token(OTHER, 2), at(11))
    });
    refuses(&mut holder, Refusal::Unknown, |h| {
        h.confirm(token(OTHER, 9), at(11))
    });
    holder.expire().unwrap();
    refuses(&mut holder, Refusal::Unknown, |h| {
        h.reacquire(token(OTHER, 9), at(12))
    });
    refuses(&mut holder, Refusal::Unknown, |h| {
        h.reacquire(token(0, 9), at(12))
    });
    assert_eq!(
        holder.reacquire(token(SCOPE, 3), at(12)),
        Ok(Recording::Recorded)
    );
}

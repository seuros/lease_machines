//! Tokens, and the fence at the protected resource.
use core::cmp::Ordering;

use periscope_machines::SEQUENCE_MAX;

use super::*;

#[test]
fn a_token_is_a_scope_above_a_positive_sequence() {
    assert_eq!(FencingToken::from_raw(0), None);
    // A scope alone is "nothing issued yet", not a token.
    assert_eq!(FencingToken::from_raw(scoped(SCOPE, 0)), None);
    let first = FencingToken::from_raw(1).unwrap();
    assert_eq!((first.raw(), first.scope()), (1, 0));
    let scoped_token = token(SCOPE, 9);
    assert_eq!(scoped_token.raw(), scoped(SCOPE, 9));
    assert_eq!(scoped_token.scope(), SCOPE);
    let last = FencingToken::from_raw(u64::MAX).unwrap();
    assert_eq!(last, token(u16::MAX, SEQUENCE_MAX));
}

#[test]
fn tokens_are_ordered_within_a_scope_and_not_across() {
    let (old, new) = (token(SCOPE, 3), token(SCOPE, 4));
    assert_eq!(old.order(new), Some(Ordering::Less));
    assert_eq!(new.order(old), Some(Ordering::Greater));
    assert_eq!(new.order(new), Some(Ordering::Equal));
    assert!(new.is_newer_than(old));
    assert!(!old.is_newer_than(new));
    assert!(!new.is_newer_than(new));
    // A bigger raw number from another scope is not newer, and not older.
    let foreign = token(OTHER, 1);
    assert!(foreign.raw() > new.raw());
    assert_eq!(foreign.order(new), None);
    assert_eq!(new.order(foreign), None);
    assert!(!foreign.is_newer_than(new));
    assert!(!new.is_newer_than(foreign));
}

#[test]
fn the_fence_says_whether_a_token_advanced_it_or_equalled_it() {
    let mut fence = Fence::with_scope(SCOPE);
    assert_eq!(fence.scope(), SCOPE);
    assert_eq!(fence.highest(), None);
    assert_eq!(fence.accept(token(SCOPE, 3)), Ok(Accepting::Advanced));
    repeats(&mut fence, Accepting::Current, |f| {
        f.accept(token(SCOPE, 3))
    });
    assert_eq!(fence.accept(token(SCOPE, 5)), Ok(Accepting::Advanced));
    refuses(&mut fence, Refusal::StaleToken, |f| {
        f.accept(token(SCOPE, 4))
    });
    refuses(&mut fence, Refusal::StaleToken, |f| {
        f.accept(token(SCOPE, 3))
    });
    assert_eq!(fence.highest(), Some(token(SCOPE, 5)));
}

#[test]
fn the_fence_refuses_every_other_scope_whatever_the_number() {
    let mut fence = Fence::with_scope(SCOPE);
    // The first token does not pick the fence's scope: the fence was built
    // for one, and a stray token cannot capture it.
    refuses(&mut fence, Refusal::Unknown, |f| f.accept(token(OTHER, 1)));
    refuses(&mut fence, Refusal::Unknown, |f| f.accept(token(0, 1)));
    assert_eq!(fence.accept(token(SCOPE, 2)), Ok(Accepting::Advanced));
    for foreign in [
        token(OTHER, 1),
        token(OTHER, 2),
        token(OTHER, 3),
        token(0, 3),
        token(u16::MAX, SEQUENCE_MAX),
    ] {
        refuses(&mut fence, Refusal::Unknown, |f| f.accept(foreign));
    }
    // Scope zero is a scope like any other.
    let mut zero = Fence::new();
    assert_eq!(zero, Fence::default());
    assert_eq!(zero.scope(), 0);
    refuses(&mut zero, Refusal::Unknown, |f| f.accept(token(SCOPE, 1)));
    assert_eq!(zero.accept(token(0, 1)), Ok(Accepting::Advanced));
}

#[test]
fn a_resumed_fence_keeps_its_high_water_and_its_scope() {
    let mut original = Fence::with_scope(SCOPE);
    original.accept(token(SCOPE, 9)).unwrap();
    let mut resumed = Fence::resumed(SCOPE, original.highest()).unwrap();
    assert_eq!(resumed, original);
    refuses(&mut resumed, Refusal::StaleToken, |f| {
        f.accept(token(SCOPE, 8))
    });
    repeats(&mut resumed, Accepting::Current, |f| {
        f.accept(token(SCOPE, 9))
    });
    assert_eq!(resumed.accept(token(SCOPE, 10)), Ok(Accepting::Advanced));
    // The scope is the caller's: a saved mark of another scope is refused.
    assert_eq!(Fence::resumed(OTHER, original.highest()), None);
    assert_eq!(Fence::resumed(0, original.highest()), None);
    assert_eq!(Fence::resumed(SCOPE, None), Some(Fence::with_scope(SCOPE)));
}

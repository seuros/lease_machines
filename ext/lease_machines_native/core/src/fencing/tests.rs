#![allow(clippy::expect_used)] // Failed test assumptions should panic.

use super::*;

fn token(value: u64) -> FencingToken {
    FencingToken::new(value).expect("test token is positive")
}

#[test]
fn tokens_are_positive_and_never_wrap() {
    assert_eq!(FencingToken::new(0), None);
    assert_eq!(FencingToken::next(None), Some(token(1)));
    assert_eq!(FencingToken::next(Some(token(u64::MAX))), None);
}

#[test]
fn resource_accepts_equal_or_newer_and_rejects_stale() {
    let mut fence = Fence::new(None);
    assert!(fence.accept(token(3)));
    assert!(fence.accept(token(3)));
    assert!(fence.accept(token(5)));
    assert!(!fence.accept(token(4)));
    assert_eq!(fence.highest(), Some(token(5)));
}

#[test]
fn restored_resource_keeps_its_high_water() {
    let mut original = Fence::new(None);
    assert!(original.accept(token(9)));
    let mut restored = Fence::new(original.highest());
    assert!(!restored.accept(token(8)));
    assert!(restored.accept(token(9)));
    assert!(restored.accept(token(10)));
}

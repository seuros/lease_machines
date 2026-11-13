#![allow(clippy::unwrap_used)] // Failed test assumptions should panic.
use core::{fmt::Debug, time::Duration};

use periscope_machines::scoped;

use super::*;

mod authority;
mod checkpoint;
mod clock;
mod fence;
mod holder;
mod model;
mod random;
mod scope;

pub const FIRST: OwnerId = OwnerId::new(1);
pub const SECOND: OwnerId = OwnerId::new(2);
/// A non-zero scope: nothing in these tests may depend on scope zero.
pub const SCOPE: u16 = 7;
pub const OTHER: u16 = 8;

pub fn at(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

/// Token `sequence` of `scope`, rebuilt as if it had crossed a wire.
pub fn token(scope: u16, sequence: u64) -> FencingToken {
    FencingToken::from_raw(scoped(scope, sequence)).unwrap()
}

/// The call is refused with `refusal` and leaves `subject` exactly as it was.
pub fn refuses<S, T>(
    subject: &mut S,
    refusal: Refusal,
    call: impl FnOnce(&mut S) -> Result<T, Refusal>,
) where
    S: Clone + PartialEq + Debug,
    T: Debug + PartialEq,
{
    let before = subject.clone();
    assert_eq!(call(subject), Err(refusal));
    assert_eq!(*subject, before, "a refusal changed something");
}

/// The call is honoured with `answer` and still leaves `subject` as it was.
pub fn repeats<S, T>(subject: &mut S, answer: T, call: impl FnOnce(&mut S) -> Result<T, Refusal>)
where
    S: Clone + PartialEq + Debug,
    T: Debug + PartialEq,
{
    let before = subject.clone();
    assert_eq!(call(subject), Ok(answer));
    assert_eq!(*subject, before, "an honoured repeat changed something");
}

//! Thin binding to the lease authority, fence and holder; no storage, clocks
//! or resource I/O.
mod authority;
mod classes;
mod fence;
mod holder;
mod init;
mod names;
mod values;

use std::time::Duration;

use lease_machines::FencingToken;
use magnus::{Error, Integer, Ruby, TryConvert, Value};

const NANOS_PER_SECOND: u128 = 1_000_000_000;

fn integer(value: Value, what: &str) -> Result<Integer, Error> {
    Integer::from_value(value).ok_or_else(|| {
        Error::new(
            Ruby::get_with(value).exception_type_error(),
            format!("{what} must be an Integer"),
        )
    })
}

fn out_of_range(value: Value, message: &'static str) -> Error {
    Error::new(Ruby::get_with(value).exception_range_error(), message)
}

/// An Integer `T` holds: `TypeError` for a non-Integer, `RangeError` (with
/// `range`) for one outside `T`.
fn unsigned<T: TryConvert>(value: Value, what: &str, range: &'static str) -> Result<T, Error> {
    integer(value, what)?;
    T::try_convert(value).map_err(|_| out_of_range(value, range))
}

/// An unsigned 64-bit Integer: an owner, a count.
#[derive(Clone, Copy)]
struct Number(u64);

impl TryConvert for Number {
    fn try_convert(value: Value) -> Result<Self, Error> {
        unsigned(value, "value", "value must be in 0..=2**64-1").map(Self)
    }
}

/// A scope: an Integer in `0..=65535`.
#[derive(Clone, Copy)]
struct Scope(u16);

impl TryConvert for Scope {
    fn try_convert(value: Value) -> Result<Self, Error> {
        unsigned(value, "scope", "scope must be in 0..=65535").map(Self)
    }
}

/// An instant or a span: Integer nanoseconds, anything a `Duration` holds.
#[derive(Clone, Copy)]
struct Nanos(Duration);

impl TryConvert for Nanos {
    fn try_convert(value: Value) -> Result<Self, Error> {
        let range = "time must be nanoseconds in 0..2**64 seconds";
        let nanos = integer(value, "time in nanoseconds")?
            .to_u128()
            .map_err(|_| out_of_range(value, range))?;
        let seconds =
            u64::try_from(nanos / NANOS_PER_SECOND).map_err(|_| out_of_range(value, range))?;
        let rest =
            u32::try_from(nanos % NANOS_PER_SECOND).map_err(|_| out_of_range(value, range))?;
        Ok(Self(Duration::new(seconds, rest)))
    }
}

/// A fencing token as its raw Integer: scope above a positive sequence.
#[derive(Clone, Copy)]
struct Token(FencingToken);

impl TryConvert for Token {
    fn try_convert(value: Value) -> Result<Self, Error> {
        let range = "token must be in 1..=2**64-1 with a positive sequence number";
        let raw = unsigned(value, "token", range)?;
        FencingToken::from_raw(raw)
            .map(Self)
            .ok_or_else(|| out_of_range(value, range))
    }
}

/// `LeaseMachines.native_raw`: what `FencingToken.new` validates.
const fn validate_raw(_raw: Token) {}

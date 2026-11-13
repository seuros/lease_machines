//! Thin binding to the lease authority, fence and holder; no storage, clocks
//! or resource I/O.
mod authority;
mod fence;
mod holder;
mod init;
mod names;

use std::time::Duration;

use lease_machines::{FencingToken, Lease, OwnerId};
use magnus::{Error, Integer, IntoValue, Ruby, TryConvert, Value};

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

/// An unsigned 64-bit Integer: an owner, a count.
#[derive(Clone, Copy)]
struct Number(u64);

impl TryConvert for Number {
    fn try_convert(value: Value) -> Result<Self, Error> {
        integer(value, "value")?;
        u64::try_convert(value)
            .map(Self)
            .map_err(|_| out_of_range(value, "value must be in 0..=2**64-1"))
    }
}

/// A scope: an Integer in `0..=65535`.
#[derive(Clone, Copy)]
struct Scope(u16);

impl TryConvert for Scope {
    fn try_convert(value: Value) -> Result<Self, Error> {
        integer(value, "scope")?;
        u16::try_convert(value)
            .map(Self)
            .map_err(|_| out_of_range(value, "scope must be in 0..=65535"))
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
        integer(value, "token")?;
        let raw = u64::try_convert(value).map_err(|_| out_of_range(value, range))?;
        FencingToken::from_raw(raw)
            .map(Self)
            .ok_or_else(|| out_of_range(value, range))
    }
}

/// A lease as `[owner, token, expires_at]`.
type Row = (Number, Token, Nanos);

const fn lease(row: Row) -> Lease {
    Lease::new(OwnerId::new(row.0.0), row.1.0, row.2.0)
}

fn row(ruby: &Ruby, lease: Lease) -> Value {
    (
        lease.owner().value(),
        lease.token().raw(),
        lease.expires_at().as_nanos(),
    )
        .into_value_with(ruby)
}

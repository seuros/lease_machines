//! Thin binding to the lease authority, fence and holder; no storage, clocks
//! or resource I/O.
mod authority;
mod classes;
mod fence;
mod holder;
mod init;
mod names;
mod values;

use lease_machines::FencingToken;
use magnus::{Error, Ruby, Value};
use matryona_machines::ruby::integer;

/// A fencing token as its raw Integer: scope above a positive sequence.
fn raw_token(value: Value) -> Result<FencingToken, Error> {
    FencingToken::from_raw(integer(value, "token")?).ok_or_else(|| {
        Error::new(
            Ruby::get_with(value).exception_range_error(),
            "token must be in 1..=2**64-1 with a positive sequence number",
        )
    })
}

/// `LeaseMachines.native_raw`: what `FencingToken.new` validates.
fn validate_raw(raw: Value) -> Result<(), Error> {
    raw_token(raw).map(drop)
}

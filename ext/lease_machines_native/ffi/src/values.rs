//! Values read from Ruby and built for it.
//!
//! Every token, lease and checkpoint the binding reads goes through
//! [`own_or_sibling`]: this gem's own class read from the struct, a sibling
//! gem's value of the same shape read by its public readers, a subclass
//! refused. A cadence is this gem's own only. Each field is then converted
//! and the value rebuilt through the core constructor, so a forged value
//! raises before it reaches the core.
use lease_machines::{Cadence, Checkpoint, FencingToken, Lease, OwnerId};
use magnus::{Error, Ruby, Value, prelude::*};
use matryona_machines::ruby::{
    Class, exact_fields, instant, integer, nanoseconds, own_or_sibling_with,
};

use crate::classes::classes;
use crate::{authority, holder, raw_token};

/// The fields of exactly this gem's `class`, or of a sibling gem's value
/// through its public readers; `TypeError` naming both for anything else, a
/// subclass of `class` included. Runs Ruby on the sibling path, so it is
/// called while converting arguments, before any core call.
fn own_or_sibling<const N: usize>(
    ruby: &Ruby,
    value: Value,
    class: Class,
    readers: [&str; N],
) -> Result<[Value; N], Error> {
    let own = class.get(ruby);
    own_or_sibling_with(ruby, value, own, readers, || {
        format!("expected a {own} or a value with #{}", readers.join(", #"))
    })
}

/// A token as the core's: this gem's `FencingToken` or a sibling's.
pub fn token(ruby: &Ruby, value: Value) -> Result<FencingToken, Error> {
    let [raw] = own_or_sibling(ruby, value, classes(ruby)?.token, ["raw"])?;
    raw_token(raw)
}

/// A lease as the core's: this gem's `Lease` or a sibling's.
pub fn lease(ruby: &Ruby, value: Value) -> Result<Lease, Error> {
    let readers = ["owner", "token", "expires_at"];
    let [owner, held, expires_at] = own_or_sibling(ruby, value, classes(ruby)?.lease, readers)?;
    let owner = OwnerId::new(integer(owner, "owner")?);
    let held = token(ruby, held)?;
    Ok(Lease::new(owner, held, instant(expires_at, "expires_at")?))
}

/// A checkpoint as the core's: this gem's `Checkpoint` or a sibling's.
pub fn checkpoint(ruby: &Ruby, value: Value) -> Result<Checkpoint, Error> {
    let readers = ["scope", "issued", "lease", "observed"];
    let [scope, issued, held, observed] =
        own_or_sibling(ruby, value, classes(ruby)?.checkpoint, readers)?;
    let (scope, issued) = (integer(scope, "scope")?, integer(issued, "issued")?);
    let held = if held.is_nil() {
        None
    } else {
        Some(lease(ruby, held)?)
    };
    authority::checkpoint(ruby, scope, issued, held, instant(observed, "observed")?)
}

/// A cadence as the core's: only this gem's `Cadence`.
pub fn cadence(ruby: &Ruby, value: Value) -> Result<Cadence, Error> {
    let class = classes(ruby)?.cadence.get(ruby);
    let message = "expected a LeaseMachines::Cadence";
    let [ttl, renew_after, warn_before] = exact_fields::<3>(ruby, value, class, message)?;
    timing(ruby, ttl, renew_after, warn_before)
}

/// `Cadence::new` of three spans, converted in order.
fn timing(
    ruby: &Ruby,
    ttl: Value,
    renew_after: Value,
    warn_before: Value,
) -> Result<Cadence, Error> {
    let ttl = instant(ttl, "ttl")?;
    let renew_after = instant(renew_after, "renew_after")?;
    holder::cadence(ruby, ttl, renew_after, instant(warn_before, "warn_before")?)
}

/// A `LeaseMachines::FencingToken` of `token`, built by its `new`.
pub fn token_object(ruby: &Ruby, token: FencingToken) -> Result<Value, Error> {
    let class = classes(ruby)?.token.get(ruby);
    class.funcall("new", (token.raw(),))
}

/// A `LeaseMachines::Lease` of `lease`, built by its `new`.
pub fn lease_object(ruby: &Ruby, lease: Lease) -> Result<Value, Error> {
    let class = classes(ruby)?.lease.get(ruby);
    let held = token_object(ruby, lease.token())?;
    let expires_at = nanoseconds(ruby, lease.expires_at());
    class.funcall("new", (lease.owner().value(), held, expires_at))
}

/// `LeaseMachines.native_lease`: what `Lease.new` validates. Answers the
/// token as this gem's own, to keep in place of the one given.
pub fn validate_lease(
    ruby: &Ruby,
    owner: Value,
    held: Value,
    expires_at: Value,
) -> Result<Value, Error> {
    integer::<u64>(owner, "owner")?;
    let held = token(ruby, held)?;
    instant(expires_at, "expires_at")?;
    token_object(ruby, held)
}

/// `LeaseMachines.native_checkpoint`: what `Checkpoint.new` validates.
/// Answers the lease as this gem's own (or nil), to keep in place of the one
/// given.
pub fn validate_checkpoint(
    ruby: &Ruby,
    scope: Value,
    issued: Value,
    held: Option<Value>,
    observed: Value,
) -> Result<Option<Value>, Error> {
    let (scope, issued) = (integer(scope, "scope")?, integer(issued, "issued")?);
    let observed = instant(observed, "observed")?;
    let held = held.map(|held| lease(ruby, held)).transpose()?;
    authority::checkpoint(ruby, scope, issued, held, observed)?;
    held.map(|held| lease_object(ruby, held)).transpose()
}

/// `LeaseMachines.native_cadence`: what `Cadence.new` validates.
pub fn validate_cadence(
    ruby: &Ruby,
    ttl: Value,
    renew_after: Value,
    warn_before: Value,
) -> Result<(), Error> {
    timing(ruby, ttl, renew_after, warn_before).map(drop)
}

//! Values read from Ruby and built for it.
//!
//! Every token, lease, checkpoint and cadence the binding reads goes through
//! [`fields`]: this gem's own class read from the struct, a sibling gem's
//! value of the same shape read by its readers, a subclass refused. Each
//! field is then converted and the value rebuilt through the core
//! constructor, so a forged value raises before it reaches the core.
use lease_machines::{Cadence, Checkpoint, FencingToken, Lease, OwnerId};
use magnus::{Error, Ruby, TryConvert, Value, prelude::*};

use crate::classes::{Origin, classes, fields};
use crate::{Nanos, Number, Scope, Token, authority, holder};

/// A token as the core's: this gem's `FencingToken` or a sibling's.
pub fn token(ruby: &Ruby, value: Value) -> Result<FencingToken, Error> {
    let [raw] = fields(ruby, value, classes(ruby)?.token, ["raw"], Origin::Sibling)?;
    Ok(Token::try_convert(raw)?.0)
}

/// A lease as the core's: this gem's `Lease` or a sibling's.
pub fn lease(ruby: &Ruby, value: Value) -> Result<Lease, Error> {
    let readers = ["owner", "token", "expires_at"];
    let [owner, held, expires_at] =
        fields(ruby, value, classes(ruby)?.lease, readers, Origin::Sibling)?;
    let owner = OwnerId::new(Number::try_convert(owner)?.0);
    let held = token(ruby, held)?;
    Ok(Lease::new(owner, held, Nanos::try_convert(expires_at)?.0))
}

/// A checkpoint as the core's: this gem's `Checkpoint` or a sibling's.
pub fn checkpoint(ruby: &Ruby, value: Value) -> Result<Checkpoint, Error> {
    let readers = ["scope", "issued", "lease", "observed"];
    let [scope, issued, held, observed] = fields(
        ruby,
        value,
        classes(ruby)?.checkpoint,
        readers,
        Origin::Sibling,
    )?;
    let (scope, issued) = (Scope::try_convert(scope)?, Number::try_convert(issued)?);
    let held = if held.is_nil() {
        None
    } else {
        Some(lease(ruby, held)?)
    };
    authority::checkpoint(ruby, scope, issued, held, Nanos::try_convert(observed)?)
}

/// A cadence as the core's: only this gem's `Cadence`.
pub fn cadence(ruby: &Ruby, value: Value) -> Result<Cadence, Error> {
    let readers = ["ttl", "renew_after", "warn_before"];
    let [ttl, renew_after, warn_before] =
        fields(ruby, value, classes(ruby)?.cadence, readers, Origin::Own)?;
    let timing = (
        Nanos::try_convert(ttl)?,
        Nanos::try_convert(renew_after)?,
        Nanos::try_convert(warn_before)?,
    );
    holder::cadence(ruby, timing)
}

/// A token argument, read by [`token`].
#[derive(Clone, Copy)]
pub struct TokenValue(pub FencingToken);

impl TryConvert for TokenValue {
    fn try_convert(value: Value) -> Result<Self, Error> {
        token(&Ruby::get_with(value), value).map(Self)
    }
}

/// A checkpoint argument, read by [`checkpoint`].
pub struct CheckpointValue(pub Checkpoint);

impl TryConvert for CheckpointValue {
    fn try_convert(value: Value) -> Result<Self, Error> {
        checkpoint(&Ruby::get_with(value), value).map(Self)
    }
}

/// A cadence argument, read by [`cadence`].
#[derive(Clone, Copy)]
pub struct CadenceValue(pub Cadence);

impl TryConvert for CadenceValue {
    fn try_convert(value: Value) -> Result<Self, Error> {
        cadence(&Ruby::get_with(value), value).map(Self)
    }
}

/// A `LeaseMachines::FencingToken` of `token`, built by its `new`.
pub fn token_object(ruby: &Ruby, token: FencingToken) -> Result<Value, Error> {
    let class = ruby.get_inner(classes(ruby)?.token);
    class.funcall("new", (token.raw(),))
}

/// A `LeaseMachines::Lease` of `lease`, built by its `new`.
pub fn lease_object(ruby: &Ruby, lease: Lease) -> Result<Value, Error> {
    let class = ruby.get_inner(classes(ruby)?.lease);
    let held = token_object(ruby, lease.token())?;
    let fields = (lease.owner().value(), held, lease.expires_at().as_nanos());
    class.funcall("new", fields)
}

/// `LeaseMachines.native_lease`: what `Lease.new` validates. Answers the
/// token as this gem's own, to keep in place of the one given.
pub fn validate_lease(
    ruby: &Ruby,
    _owner: Number,
    held: TokenValue,
    _expires_at: Nanos,
) -> Result<Value, Error> {
    token_object(ruby, held.0)
}

/// `LeaseMachines.native_checkpoint`: what `Checkpoint.new` validates.
/// Answers the lease as this gem's own (or nil), to keep in place of the one
/// given.
pub fn validate_checkpoint(
    ruby: &Ruby,
    scope: Scope,
    issued: Number,
    held: Option<Value>,
    observed: Nanos,
) -> Result<Option<Value>, Error> {
    let held = held.map(|held| lease(ruby, held)).transpose()?;
    authority::checkpoint(ruby, scope, issued, held, observed)?;
    held.map(|held| lease_object(ruby, held)).transpose()
}

/// `LeaseMachines.native_cadence`: what `Cadence.new` validates.
pub fn validate_cadence(
    ruby: &Ruby,
    ttl: Nanos,
    renew_after: Nanos,
    warn_before: Nanos,
) -> Result<(), Error> {
    holder::cadence(ruby, (ttl, renew_after, warn_before)).map(drop)
}

//! `LeaseMachines::LeaseAuthority` and the checkpoints it trades in.
use std::{cell::RefCell, time::Duration};

use lease_machines::{Checkpoint, Lease, OwnerId, Refusal};
use magnus::{Error, IntoValue, Ruby, Value, prelude::*, typed_data::Obj};
use matryona_machines::ruby::{Symbols, instant, integer, keep, nanoseconds};

use crate::names;
use crate::values::{self, lease_object};

type Core = lease_machines::LeaseAuthority;

/// `Checkpoint::from_parts`, or the `RangeError` its `None` stands for.
pub fn checkpoint(
    ruby: &Ruby,
    scope: u16,
    issued: u64,
    held: Option<Lease>,
    observed: Duration,
) -> Result<Checkpoint, Error> {
    Checkpoint::from_parts(scope, issued, held, observed).ok_or_else(|| {
        Error::new(
            ruby.exception_range_error(),
            "issued is beyond the scope's token space, or the lease is not the grant \
             that history ends with",
        )
    })
}

fn owner(value: Value) -> Result<OwnerId, Error> {
    integer(value, "owner").map(OwnerId::new)
}

/// An owner, a `now` and a `ttl`, converted in that order.
fn times(owner: Value, now: Value, ttl: Value) -> Result<(OwnerId, Duration, Duration), Error> {
    Ok((
        self::owner(owner)?,
        instant(now, "now")?,
        instant(ttl, "ttl")?,
    ))
}

/// A lease as `[owner, token, expires_at]`, for Ruby to build a `Lease` of.
fn row(ruby: &Ruby, lease: Lease) -> Value {
    (
        lease.owner().value(),
        lease.token().raw(),
        nanoseconds(ruby, lease.expires_at()),
    )
        .into_value_with(ruby)
}

#[magnus::wrap(class = "LeaseMachines::LeaseAuthority", free_immediately, size)]
pub struct LeaseAuthority(RefCell<Core>);

impl LeaseAuthority {
    pub fn with_scope(scope: Value) -> Result<Self, Error> {
        Ok(Self(RefCell::new(Core::with_scope(integer(
            scope, "scope",
        )?))))
    }

    pub fn restored(ruby: &Ruby, scope: Value, saved: Value, clock: Value) -> Result<Self, Error> {
        let (scope, saved) = (integer(scope, "scope")?, values::checkpoint(ruby, saved)?);
        Core::restored(scope, saved, names::ClockPolicies::parse(ruby, clock)?)
            .map(|core| Self(RefCell::new(core)))
            .ok_or_else(|| {
                Error::new(
                    ruby.exception_range_error(),
                    "the checkpoint is of another scope",
                )
            })
    }

    pub fn with_clock_policy(ruby: &Ruby, this: Obj<Self>, clock: Value) -> Result<(), Error> {
        this.check_frozen()?;
        let clock = names::ClockPolicies::parse(ruby, clock)?;
        let mut core = this.0.borrow_mut();
        *core = core.clone().with_clock_policy(clock);
        Ok(())
    }

    pub fn clock_policy(ruby: &Ruby, this: &Self) -> magnus::Symbol {
        names::ClockPolicies::symbol(ruby, this.0.borrow().clock_policy())
    }

    pub fn scope(&self) -> u16 {
        self.0.borrow().scope()
    }

    pub fn issued(&self) -> u64 {
        self.0.borrow().issued()
    }

    pub fn checkpoint(ruby: &Ruby, this: &Self) -> Value {
        let saved = this.0.borrow().checkpoint();
        (
            saved.scope(),
            saved.issued(),
            saved.lease().map(|held| row(ruby, held)),
            nanoseconds(ruby, saved.observed()),
        )
            .into_value_with(ruby)
    }

    /// Every decision: a `Lease` when honoured, or the refusal symbol.
    /// Everything that can raise happens before the core is called; the
    /// `Lease` is built while the decision is held on a copy, kept only once
    /// it exists (clone-then-keep).
    fn decide(
        ruby: &Ruby,
        this: Obj<Self>,
        call: impl FnOnce(&mut Core) -> Result<Lease, Refusal>,
    ) -> Result<Value, Error> {
        keep(ruby, this, &this.0, call, |decision| match decision {
            Ok(granted) => lease_object(ruby, granted),
            Err(refusal) => Ok(names::Refusals::symbol(ruby, refusal).as_value()),
        })
    }

    pub fn acquire(
        ruby: &Ruby,
        this: Obj<Self>,
        owner: Value,
        now: Value,
        ttl: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let (owner, now, ttl) = times(owner, now, ttl)?;
        Self::decide(ruby, this, |core| core.acquire(owner, now, ttl))
    }

    pub fn reacquire(
        ruby: &Ruby,
        this: Obj<Self>,
        owner: Value,
        now: Value,
        ttl: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let (owner, now, ttl) = times(owner, now, ttl)?;
        Self::decide(ruby, this, |core| core.reacquire(owner, now, ttl))
    }

    pub fn renew(
        ruby: &Ruby,
        this: Obj<Self>,
        owner: Value,
        token: Value,
        now: Value,
        ttl: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let (owner, token) = (self::owner(owner)?, values::token(ruby, token)?);
        let (now, ttl) = (instant(now, "now")?, instant(ttl, "ttl")?);
        Self::decide(ruby, this, |core| core.renew(owner, token, now, ttl))
    }

    pub fn release(
        ruby: &Ruby,
        this: Obj<Self>,
        owner: Value,
        token: Value,
        now: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let (owner, token) = (self::owner(owner)?, values::token(ruby, token)?);
        let now = instant(now, "now")?;
        Self::decide(ruby, this, |core| core.release(owner, token, now))
    }
}

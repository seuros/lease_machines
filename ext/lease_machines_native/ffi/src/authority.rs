//! `LeaseMachines::LeaseAuthority` and the checkpoints it trades in.
use std::{cell::RefCell, time::Duration};

use lease_machines::{Lease, OwnerId, Refusal};
use magnus::{Error, Ruby, Value, prelude::*, typed_data::Obj};
use matryona_machines::ruby::{
    Symbols,
    fencing::{self, checkpoint, checkpoint_row, lease_object},
    instant, integer, keep,
};

use crate::names;
use crate::values::Family;

type Core = lease_machines::LeaseAuthority;

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

#[magnus::wrap(class = "LeaseMachines::LeaseAuthority", free_immediately, size)]
pub struct LeaseAuthority(RefCell<Core>);

impl LeaseAuthority {
    pub fn with_scope(scope: Value) -> Result<Self, Error> {
        Ok(Self(RefCell::new(Core::with_scope(integer(
            scope, "scope",
        )?))))
    }

    pub fn restored(ruby: &Ruby, scope: Value, saved: Value, clock: Value) -> Result<Self, Error> {
        let (scope, saved) = (integer(scope, "scope")?, checkpoint::<Family>(ruby, saved)?);
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
        checkpoint_row::<Family>(ruby, &this.0.borrow().checkpoint())
    }

    /// The live grant an `acquire` at `now` for `ttl` would meet: a
    /// `Lease`, `nil` when it would grant, or the refusal symbol. A query
    /// records nothing, so the `Lease` is built once the borrow has ended,
    /// with no copy to keep.
    pub fn incumbent(ruby: &Ruby, this: &Self, now: Value, ttl: Value) -> Result<Value, Error> {
        let (now, ttl) = (instant(now, "now")?, instant(ttl, "ttl")?);
        let answer = this.0.borrow().incumbent(now, ttl);
        match answer {
            Ok(Some(live)) => lease_object::<Family>(ruby, live),
            Ok(None) => Ok(ruby.qnil().as_value()),
            Err(refusal) => Ok(names::Refusals::symbol(ruby, refusal).as_value()),
        }
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
            Ok(granted) => lease_object::<Family>(ruby, granted),
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
        let (owner, token) = (self::owner(owner)?, fencing::token::<Family>(ruby, token)?);
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
        let (owner, token) = (self::owner(owner)?, fencing::token::<Family>(ruby, token)?);
        let now = instant(now, "now")?;
        Self::decide(ruby, this, |core| core.release(owner, token, now))
    }
}

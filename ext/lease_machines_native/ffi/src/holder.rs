//! `LeaseMachines::Holder` and the cadence it is built with.
use std::{cell::RefCell, time::Duration};

use lease_machines::Cadence;
use magnus::{Error, Integer, Ruby, Symbol, Value, prelude::*, typed_data::Obj};
use matryona_machines::ruby::{Symbols, instant, nanoseconds};

use crate::{names, values};

type Core = lease_machines::Holder;

/// `Cadence::new`, or the `RangeError` its `None` stands for.
pub fn cadence(
    ruby: &Ruby,
    ttl: Duration,
    renew_after: Duration,
    warn_before: Duration,
) -> Result<Cadence, Error> {
    Cadence::new(ttl, renew_after, warn_before).ok_or_else(|| {
        Error::new(
            ruby.exception_range_error(),
            "cadence needs a positive ttl, renew_after < ttl and warn_before <= ttl",
        )
    })
}

#[magnus::wrap(class = "LeaseMachines::Holder", free_immediately, size)]
pub struct Holder(RefCell<Core>);

impl Holder {
    pub fn new(
        ruby: &Ruby,
        token: Value,
        confirmed_at: Value,
        cadence: Value,
    ) -> Result<Self, Error> {
        let token = values::token(ruby, token)?;
        let confirmed_at = instant(confirmed_at, "confirmed_at")?;
        let cadence = values::cadence(ruby, cadence)?;
        Ok(Self(RefCell::new(Core::new(token, confirmed_at, cadence))))
    }

    pub fn state(ruby: &Ruby, this: &Self) -> Symbol {
        names::HolderStates::symbol(ruby, this.0.borrow().state())
    }

    pub fn token(&self) -> u64 {
        self.0.borrow().token().raw()
    }

    pub fn cadence(ruby: &Ruby, this: &Self) -> (Integer, Integer, Integer) {
        let cadence = this.0.borrow().cadence();
        (
            nanoseconds(ruby, cadence.ttl()),
            nanoseconds(ruby, cadence.renew_after()),
            nanoseconds(ruby, cadence.warn_before()),
        )
    }

    pub fn renewal_due(&self, now: Value) -> Result<bool, Error> {
        let now = instant(now, "now")?;
        Ok(self.0.borrow().renewal_due(now))
    }

    pub fn warning_deadline(ruby: &Ruby, this: &Self) -> Option<Integer> {
        let deadline = this.0.borrow().warning_deadline();
        deadline.map(|at| nanoseconds(ruby, at))
    }

    pub fn at_risk(&self, now: Value) -> Result<bool, Error> {
        let now = instant(now, "now")?;
        Ok(self.0.borrow().at_risk(now))
    }

    pub fn refresh(ruby: &Ruby, this: Obj<Self>, now: Value) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let now = instant(now, "now")?;
        let outcome = this.0.borrow_mut().refresh(now);
        Ok(names::refreshed(ruby, outcome))
    }

    pub fn confirm(ruby: &Ruby, this: Obj<Self>, token: Value, at: Value) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let (token, at) = (values::token(ruby, token)?, instant(at, "at")?);
        let outcome = this.0.borrow_mut().confirm(token, at);
        Ok(names::recorded(ruby, outcome))
    }

    pub fn expire(ruby: &Ruby, this: Obj<Self>) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let outcome = this.0.borrow_mut().expire();
        Ok(names::recorded(ruby, outcome))
    }

    pub fn reacquire(
        ruby: &Ruby,
        this: Obj<Self>,
        token: Value,
        at: Value,
    ) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let (token, at) = (values::token(ruby, token)?, instant(at, "at")?);
        let outcome = this.0.borrow_mut().reacquire(token, at);
        Ok(names::recorded(ruby, outcome))
    }

    pub fn fence(this: Obj<Self>) -> Result<(), Error> {
        this.check_frozen()?;
        this.0.borrow_mut().fence();
        Ok(())
    }
}

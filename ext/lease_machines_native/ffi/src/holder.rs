//! `LeaseMachines::Holder` and the cadence it is built with.
use std::cell::RefCell;

use lease_machines::Cadence;
use magnus::{Error, Ruby, Symbol, prelude::*, typed_data::Obj};

use crate::values::{CadenceValue, TokenValue};
use crate::{Nanos, names};

type Core = lease_machines::Holder;

/// A cadence as `[ttl, renew_after, warn_before]`.
type Timing = (Nanos, Nanos, Nanos);

/// `Cadence::new`, or the `RangeError` its `None` stands for.
pub fn cadence(ruby: &Ruby, timing: Timing) -> Result<Cadence, Error> {
    Cadence::new(timing.0.0, timing.1.0, timing.2.0).ok_or_else(|| {
        Error::new(
            ruby.exception_range_error(),
            "cadence needs a positive ttl, renew_after < ttl and warn_before <= ttl",
        )
    })
}

#[magnus::wrap(class = "LeaseMachines::Holder", free_immediately, size)]
pub struct Holder(RefCell<Core>);

impl Holder {
    pub const fn new(token: TokenValue, confirmed_at: Nanos, cadence: CadenceValue) -> Self {
        Self(RefCell::new(Core::new(token.0, confirmed_at.0, cadence.0)))
    }

    pub fn state(ruby: &Ruby, this: &Self) -> Symbol {
        ruby.to_symbol(names::holder_state(this.0.borrow().state()))
    }

    pub fn token(&self) -> u64 {
        self.0.borrow().token().raw()
    }

    pub fn cadence(&self) -> (u128, u128, u128) {
        let cadence = self.0.borrow().cadence();
        (
            cadence.ttl().as_nanos(),
            cadence.renew_after().as_nanos(),
            cadence.warn_before().as_nanos(),
        )
    }

    pub fn renewal_due(&self, now: Nanos) -> bool {
        self.0.borrow().renewal_due(now.0)
    }

    pub fn warning_deadline(&self) -> Option<u128> {
        self.0.borrow().warning_deadline().map(|at| at.as_nanos())
    }

    pub fn at_risk(&self, now: Nanos) -> bool {
        self.0.borrow().at_risk(now.0)
    }

    pub fn refresh(ruby: &Ruby, this: Obj<Self>, now: Nanos) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let outcome = this.0.borrow_mut().refresh(now.0);
        Ok(names::refreshed(ruby, outcome))
    }

    pub fn confirm(
        ruby: &Ruby,
        this: Obj<Self>,
        token: TokenValue,
        at: Nanos,
    ) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let outcome = this.0.borrow_mut().confirm(token.0, at.0);
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
        token: TokenValue,
        at: Nanos,
    ) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let outcome = this.0.borrow_mut().reacquire(token.0, at.0);
        Ok(names::recorded(ruby, outcome))
    }

    pub fn fence(this: Obj<Self>) -> Result<(), Error> {
        this.check_frozen()?;
        this.0.borrow_mut().fence();
        Ok(())
    }
}

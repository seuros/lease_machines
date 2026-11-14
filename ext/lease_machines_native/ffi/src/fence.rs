//! `LeaseMachines::Fence`, and what `FencingToken` asks of the core.
use std::{cell::RefCell, cmp::Ordering};

use magnus::{Error, Ruby, Symbol, Value, prelude::*, typed_data::Obj};
use matryona_machines::ruby::integer;

use crate::{names, values::token};

type Core = lease_machines::Fence;

/// `FencingToken#scope`; reading the token is what validates it.
pub fn token_scope(ruby: &Ruby, held: Value) -> Result<u16, Error> {
    token(ruby, held).map(lease_machines::FencingToken::scope)
}

/// `FencingToken#order`: -1, 0, 1, or `nil` across scopes.
pub fn token_order(ruby: &Ruby, held: Value, other: Value) -> Result<Option<i8>, Error> {
    let (held, other) = (token(ruby, held)?, token(ruby, other)?);
    Ok(held.order(other).map(|order| match order {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }))
}

/// `FencingToken#newer_than?`.
pub fn token_newer(ruby: &Ruby, held: Value, other: Value) -> Result<bool, Error> {
    let (held, other) = (token(ruby, held)?, token(ruby, other)?);
    Ok(held.is_newer_than(other))
}

#[magnus::wrap(class = "LeaseMachines::Fence", free_immediately, size)]
pub struct Fence(RefCell<Core>);

impl Fence {
    pub fn with_scope(scope: Value) -> Result<Self, Error> {
        Ok(Self(RefCell::new(Core::with_scope(integer(
            scope, "scope",
        )?))))
    }

    pub fn resumed(ruby: &Ruby, scope: Value, highest: Option<Value>) -> Result<Self, Error> {
        let scope = integer(scope, "scope")?;
        let highest = highest.map(|held| token(ruby, held)).transpose()?;
        Core::resumed(scope, highest)
            .map(|core| Self(RefCell::new(core)))
            .ok_or_else(|| {
                Error::new(
                    ruby.exception_range_error(),
                    "highest is a token of another scope",
                )
            })
    }

    pub fn scope(&self) -> u16 {
        self.0.borrow().scope()
    }

    pub fn highest(&self) -> Option<u64> {
        self.0
            .borrow()
            .highest()
            .map(lease_machines::FencingToken::raw)
    }

    pub fn accept(ruby: &Ruby, this: Obj<Self>, held: Value) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let held = token(ruby, held)?;
        let outcome = this.0.borrow_mut().accept(held);
        Ok(names::accepted(ruby, outcome))
    }
}

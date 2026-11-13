//! `LeaseMachines::Fence`, and what `FencingToken` asks of the core.
use std::{cell::RefCell, cmp::Ordering};

use magnus::{Error, Ruby, Symbol, prelude::*, typed_data::Obj};

use crate::{Scope, Token, names};

type Core = lease_machines::Fence;

/// `FencingToken#scope`; converting the token is what validates it.
pub const fn token_scope(token: Token) -> u16 {
    token.0.scope()
}

/// `FencingToken#order`: -1, 0, 1, or `nil` across scopes.
pub fn token_order(token: Token, other: Token) -> Option<i8> {
    token.0.order(other.0).map(|order| match order {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    })
}

/// `FencingToken#newer_than?`.
pub const fn token_newer(token: Token, other: Token) -> bool {
    token.0.is_newer_than(other.0)
}

#[magnus::wrap(class = "LeaseMachines::Fence", free_immediately, size)]
pub struct Fence(RefCell<Core>);

impl Fence {
    pub const fn with_scope(scope: Scope) -> Self {
        Self(RefCell::new(Core::with_scope(scope.0)))
    }

    pub fn resumed(ruby: &Ruby, scope: Scope, highest: Option<Token>) -> Result<Self, Error> {
        Core::resumed(scope.0, highest.map(|token| token.0))
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

    pub fn accept(ruby: &Ruby, this: Obj<Self>, token: Token) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let outcome = this.0.borrow_mut().accept(token.0);
        Ok(names::accepted(ruby, outcome))
    }
}

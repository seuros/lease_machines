//! `LeaseMachines::Fence`. What `FencingToken` asks of the core is
//! `matryona_machines::ruby::fencing`'s, over [`Family`].
use std::cell::RefCell;

use magnus::{Error, Ruby, Symbol, Value, prelude::*, typed_data::Obj};
use matryona_machines::ruby::{fencing::token, integer};

use crate::{names, values::Family};

type Core = lease_machines::Fence;

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
        let highest = highest
            .map(|held| token::<Family>(ruby, held))
            .transpose()?;
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
        let held = token::<Family>(ruby, held)?;
        let outcome = this.0.borrow_mut().accept(held);
        Ok(names::accepted(ruby, outcome))
    }
}

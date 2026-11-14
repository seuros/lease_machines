//! `LeaseMachines::LeaseAuthority` and the checkpoints it trades in.
use std::cell::RefCell;

use lease_machines::{Checkpoint, Lease, OwnerId, Refusal};
use magnus::{Error, IntoValue, Ruby, Value, prelude::*, typed_data::Obj};

use crate::classes::keep;
use crate::names::{self, Policy};
use crate::values::{CheckpointValue, TokenValue, lease_object};
use crate::{Nanos, Number, Scope};

type Core = lease_machines::LeaseAuthority;

/// `Checkpoint::from_parts`, or the `RangeError` its `None` stands for.
pub fn checkpoint(
    ruby: &Ruby,
    scope: Scope,
    issued: Number,
    held: Option<Lease>,
    observed: Nanos,
) -> Result<Checkpoint, Error> {
    Checkpoint::from_parts(scope.0, issued.0, held, observed.0).ok_or_else(|| {
        Error::new(
            ruby.exception_range_error(),
            "issued is beyond the scope's token space, or the lease is not the grant \
             that history ends with",
        )
    })
}

/// A lease as `[owner, token, expires_at]`, for Ruby to build a `Lease` of.
fn row(ruby: &Ruby, lease: Lease) -> Value {
    (
        lease.owner().value(),
        lease.token().raw(),
        lease.expires_at().as_nanos(),
    )
        .into_value_with(ruby)
}

#[magnus::wrap(class = "LeaseMachines::LeaseAuthority", free_immediately, size)]
pub struct LeaseAuthority(RefCell<Core>);

impl LeaseAuthority {
    pub const fn with_scope(scope: Scope) -> Self {
        Self(RefCell::new(Core::with_scope(scope.0)))
    }

    pub fn restored(
        ruby: &Ruby,
        scope: Scope,
        saved: CheckpointValue,
        clock: Policy,
    ) -> Result<Self, Error> {
        Core::restored(scope.0, saved.0, clock.0)
            .map(|core| Self(RefCell::new(core)))
            .ok_or_else(|| {
                Error::new(
                    ruby.exception_range_error(),
                    "the checkpoint is of another scope",
                )
            })
    }

    pub fn with_clock_policy(this: Obj<Self>, clock: Policy) -> Result<(), Error> {
        this.check_frozen()?;
        let mut core = this.0.borrow_mut();
        *core = core.clone().with_clock_policy(clock.0);
        Ok(())
    }

    pub fn clock_policy(ruby: &Ruby, this: &Self) -> magnus::Symbol {
        ruby.to_symbol(names::clock_policy(this.0.borrow().clock_policy()))
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
            saved.observed().as_nanos(),
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
            Err(refusal) => Ok(ruby.to_symbol(names::refusal(refusal)).as_value()),
        })
    }

    pub fn acquire(
        ruby: &Ruby,
        this: Obj<Self>,
        owner: Number,
        now: Nanos,
        ttl: Nanos,
    ) -> Result<Value, Error> {
        Self::decide(ruby, this, |core| {
            core.acquire(OwnerId::new(owner.0), now.0, ttl.0)
        })
    }

    pub fn reacquire(
        ruby: &Ruby,
        this: Obj<Self>,
        owner: Number,
        now: Nanos,
        ttl: Nanos,
    ) -> Result<Value, Error> {
        Self::decide(ruby, this, |core| {
            core.reacquire(OwnerId::new(owner.0), now.0, ttl.0)
        })
    }

    pub fn renew(
        ruby: &Ruby,
        this: Obj<Self>,
        owner: Number,
        token: TokenValue,
        now: Nanos,
        ttl: Nanos,
    ) -> Result<Value, Error> {
        Self::decide(ruby, this, |core| {
            core.renew(OwnerId::new(owner.0), token.0, now.0, ttl.0)
        })
    }

    pub fn release(
        ruby: &Ruby,
        this: Obj<Self>,
        owner: Number,
        token: TokenValue,
        now: Nanos,
    ) -> Result<Value, Error> {
        Self::decide(ruby, this, |core| {
            core.release(OwnerId::new(owner.0), token.0, now.0)
        })
    }
}

use std::{cell::RefCell, time::Duration};

use lease_machines::{
    Checkpoint, Decision, Fence, FencingToken, Lease, LeaseAuthority, OwnerId, Rejection,
};
use magnus::typed_data::Obj;
use magnus::{Error, Integer, RArray, Ruby, TryConvert, Value, function, method, prelude::*};

#[derive(Clone, Copy)]
struct Count(u64);

impl TryConvert for Count {
    fn try_convert(value: Value) -> Result<Self, Error> {
        if Integer::from_value(value).is_none() {
            return Err(Error::new(
                Ruby::get_with(value).exception_type_error(),
                "expected an Integer",
            ));
        }
        u64::try_convert(value).map(Self)
    }
}

fn duration(ruby: &Ruby, seconds: f64) -> Result<Duration, Error> {
    Duration::try_from_secs_f64(seconds).map_err(|_| {
        Error::new(
            ruby.exception_arg_error(),
            "time must be finite, nonnegative, and representable",
        )
    })
}

fn token(ruby: &Ruby, value: Count) -> Result<FencingToken, Error> {
    FencingToken::new(value.0)
        .ok_or_else(|| Error::new(ruby.exception_arg_error(), "token must be positive"))
}

#[magnus::wrap(class = "LeaseMachines::Lease", free_immediately, size)]
struct Grant(Lease);

impl Grant {
    const fn owner(&self) -> u64 {
        self.0.owner().value()
    }
    const fn token(&self) -> u64 {
        self.0.token().value()
    }
    const fn expires_at(&self) -> f64 {
        self.0.expires_at().as_secs_f64()
    }
}

#[magnus::wrap(class = "LeaseMachines::Checkpoint", free_immediately, size)]
struct Saved(Checkpoint);

impl Saved {
    fn last_issued(&self) -> Option<u64> {
        self.0.last_issued().map(FencingToken::value)
    }
    const fn observed(&self) -> f64 {
        self.0.observed().as_secs_f64()
    }
}

fn decision(ruby: &Ruby, value: Decision) -> Result<RArray, Error> {
    let result = ruby.ary_new();
    match value {
        Decision::Granted(lease) | Decision::Renewed(lease) => {
            let name = if matches!(value, Decision::Granted(_)) {
                "granted"
            } else {
                "renewed"
            };
            result.push(ruby.to_symbol(name))?;
            result.push(Grant(lease))?;
            result.push(())?;
        }
        Decision::Released => {
            result.push(ruby.to_symbol("released"))?;
            result.push(())?;
            result.push(())?;
        }
        Decision::Rejected(reason) => {
            let name = match reason {
                Rejection::Occupied => "occupied",
                Rejection::NoLease => "no_lease",
                Rejection::WrongOwner => "wrong_owner",
                Rejection::StaleToken => "stale_token",
                Rejection::Expired => "expired",
                Rejection::ClockRegressed => "clock_regressed",
                Rejection::InvalidTtl => "invalid_ttl",
                Rejection::TokenExhausted => "token_exhausted",
                _ => {
                    return Err(Error::new(
                        ruby.exception_runtime_error(),
                        "unsupported rejection",
                    ));
                }
            };
            result.push(ruby.to_symbol("rejected"))?;
            result.push(())?;
            result.push(ruby.to_symbol(name))?;
        }
        _ => {
            return Err(Error::new(
                ruby.exception_runtime_error(),
                "unsupported decision",
            ));
        }
    }
    Ok(result)
}

#[magnus::wrap(class = "LeaseMachines::Authority", free_immediately, size)]
struct Authority(RefCell<LeaseAuthority>);

impl Authority {
    const fn new() -> Self {
        Self(RefCell::new(LeaseAuthority::new()))
    }
    const fn restore(saved: &Saved) -> Self {
        Self(RefCell::new(LeaseAuthority::restore(saved.0)))
    }
    fn checkpoint(&self) -> Saved {
        Saved(self.0.borrow().checkpoint())
    }
    fn acquire(
        ruby: &Ruby,
        this: Obj<Self>,
        owner: Count,
        now: f64,
        ttl: f64,
    ) -> Result<RArray, Error> {
        this.check_frozen()?;
        let (now, ttl) = (duration(ruby, now)?, duration(ruby, ttl)?);
        let value = this.0.borrow_mut().acquire(OwnerId::new(owner.0), now, ttl);
        decision(ruby, value)
    }
    fn renew(
        ruby: &Ruby,
        this: Obj<Self>,
        owner: Count,
        value: Count,
        now: f64,
        ttl: f64,
    ) -> Result<RArray, Error> {
        this.check_frozen()?;
        let (value, now, ttl) = (
            token(ruby, value)?,
            duration(ruby, now)?,
            duration(ruby, ttl)?,
        );
        let value = this
            .0
            .borrow_mut()
            .renew(OwnerId::new(owner.0), value, now, ttl);
        decision(ruby, value)
    }
    fn release(
        ruby: &Ruby,
        this: Obj<Self>,
        owner: Count,
        value: Count,
        now: f64,
    ) -> Result<RArray, Error> {
        this.check_frozen()?;
        let (value, now) = (token(ruby, value)?, duration(ruby, now)?);
        let value = this
            .0
            .borrow_mut()
            .release(OwnerId::new(owner.0), value, now);
        decision(ruby, value)
    }
}

#[magnus::wrap(class = "LeaseMachines::Fence", free_immediately, size)]
struct Guard(RefCell<Fence>);

impl Guard {
    fn new(ruby: &Ruby, highest: Option<Count>) -> Result<Self, Error> {
        Ok(Self(RefCell::new(Fence::new(
            highest.map(|v| token(ruby, v)).transpose()?,
        ))))
    }
    fn accept(ruby: &Ruby, this: Obj<Self>, value: Count) -> Result<bool, Error> {
        this.check_frozen()?;
        let value = token(ruby, value)?;
        Ok(this.0.borrow_mut().accept(value))
    }
    fn highest(&self) -> Option<u64> {
        self.0.borrow().highest().map(FencingToken::value)
    }
}

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("LeaseMachines")?;
    let grant = module.define_class("Lease", ruby.class_object())?;
    grant.undef_default_alloc_func();
    grant.define_method("owner", method!(Grant::owner, 0))?;
    grant.define_method("token", method!(Grant::token, 0))?;
    grant.define_method("expires_at", method!(Grant::expires_at, 0))?;
    let saved = module.define_class("Checkpoint", ruby.class_object())?;
    saved.undef_default_alloc_func();
    saved.define_method("last_issued", method!(Saved::last_issued, 0))?;
    saved.define_method("observed", method!(Saved::observed, 0))?;
    let authority = module.define_class("Authority", ruby.class_object())?;
    authority.define_singleton_method("new", function!(Authority::new, 0))?;
    authority.define_singleton_method("restore", function!(Authority::restore, 1))?;
    authority.define_method("checkpoint", method!(Authority::checkpoint, 0))?;
    authority.define_method("_acquire", method!(Authority::acquire, 3))?;
    authority.define_method("_renew", method!(Authority::renew, 4))?;
    authority.define_method("_release", method!(Authority::release, 3))?;
    let fence = module.define_class("Fence", ruby.class_object())?;
    fence.define_singleton_method("_new", function!(Guard::new, 1))?;
    fence.define_method("accept", method!(Guard::accept, 1))?;
    fence.define_method("highest", method!(Guard::highest, 0))
}

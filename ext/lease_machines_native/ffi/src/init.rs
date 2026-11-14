//! Private native entry point.
use magnus::{Error, RArray, Ruby, function, method, prelude::*};
use matryona_machines::ruby::Symbols;

use crate::authority::LeaseAuthority;
use crate::fence::{self, Fence};
use crate::holder::Holder;
use crate::{classes, names, values};

/// Every Symbol of a table, in its order, frozen.
fn every<T: Symbols>(ruby: &Ruby) -> RArray {
    let list = ruby.ary_from_iter(T::ALL.iter().map(|variant| T::symbol(ruby, *variant)));
    list.freeze();
    list
}

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("LeaseMachines")?;
    classes::resolve(ruby, module)?;
    module.const_set("REFUSALS", every::<names::Refusals>(ruby))?;
    module.const_set("HOLDER_STATES", every::<names::HolderStates>(ruby))?;
    module.const_set("CLOCK_POLICIES", every::<names::ClockPolicies>(ruby))?;
    module.define_singleton_method("native_token_scope", function!(fence::token_scope, 1))?;
    module.define_singleton_method("native_token_order", function!(fence::token_order, 2))?;
    module.define_singleton_method("native_token_newer", function!(fence::token_newer, 2))?;
    module.define_singleton_method("native_raw", function!(crate::validate_raw, 1))?;
    module.define_singleton_method("native_lease", function!(values::validate_lease, 3))?;
    module.define_singleton_method("native_cadence", function!(values::validate_cadence, 3))?;
    let checkpoint = function!(values::validate_checkpoint, 4);
    module.define_singleton_method("native_checkpoint", checkpoint)?;

    let class = module.define_class("LeaseAuthority", ruby.class_object())?;
    class.undef_default_alloc_func();
    class.define_singleton_method(
        "native_with_scope",
        function!(LeaseAuthority::with_scope, 1),
    )?;
    class.define_singleton_method("native_restored", function!(LeaseAuthority::restored, 3))?;
    class.define_method("scope", method!(LeaseAuthority::scope, 0))?;
    class.define_method("issued", method!(LeaseAuthority::issued, 0))?;
    class.define_method("clock_policy", method!(LeaseAuthority::clock_policy, 0))?;
    class.define_private_method(
        "native_with_clock_policy",
        method!(LeaseAuthority::with_clock_policy, 1),
    )?;
    class.define_private_method("native_checkpoint", method!(LeaseAuthority::checkpoint, 0))?;
    class.define_private_method("native_acquire", method!(LeaseAuthority::acquire, 3))?;
    class.define_private_method("native_reacquire", method!(LeaseAuthority::reacquire, 3))?;
    class.define_private_method("native_renew", method!(LeaseAuthority::renew, 4))?;
    class.define_private_method("native_release", method!(LeaseAuthority::release, 3))?;

    let class = module.define_class("Fence", ruby.class_object())?;
    class.undef_default_alloc_func();
    class.define_singleton_method("native_with_scope", function!(Fence::with_scope, 1))?;
    class.define_singleton_method("native_resumed", function!(Fence::resumed, 2))?;
    class.define_method("scope", method!(Fence::scope, 0))?;
    class.define_private_method("native_highest", method!(Fence::highest, 0))?;
    class.define_private_method("native_accept", method!(Fence::accept, 1))?;

    let class = module.define_class("Holder", ruby.class_object())?;
    class.undef_default_alloc_func();
    class.define_singleton_method("native_new", function!(Holder::new, 3))?;
    class.define_method("state", method!(Holder::state, 0))?;
    class.define_private_method("native_token", method!(Holder::token, 0))?;
    class.define_private_method("native_cadence", method!(Holder::cadence, 0))?;
    class.define_method("renewal_due?", method!(Holder::renewal_due, 1))?;
    class.define_method("warning_deadline", method!(Holder::warning_deadline, 0))?;
    class.define_method("at_risk?", method!(Holder::at_risk, 1))?;
    class.define_method("refresh", method!(Holder::refresh, 1))?;
    class.define_private_method("native_confirm", method!(Holder::confirm, 2))?;
    class.define_method("expire", method!(Holder::expire, 0))?;
    class.define_private_method("native_reacquire", method!(Holder::reacquire, 2))?;
    class.define_method("fence", method!(Holder::fence, 0))?;
    Ok(())
}

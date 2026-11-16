//! Private native entry point.
use magnus::{Error, Ruby, function, method, prelude::*};
use matryona_machines::ruby::{Class, Symbols, fencing};

use crate::authority::LeaseAuthority;
use crate::fence::Fence;
use crate::holder::Holder;
use crate::values::{self, Family};
use crate::{classes, names};

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("LeaseMachines")?;
    classes::resolve(ruby, module)?;
    module.const_set("REFUSALS", names::Refusals::every(ruby))?;
    module.const_set("HOLDER_STATES", names::HolderStates::every(ruby))?;
    module.const_set("CLOCK_POLICIES", names::ClockPolicies::every(ruby))?;
    let scope = function!(fencing::token_scope::<Family>, 1);
    module.define_singleton_method("native_token_scope", scope)?;
    let order = function!(fencing::token_order::<Family>, 2);
    module.define_singleton_method("native_token_order", order)?;
    let newer = function!(fencing::token_newer::<Family>, 2);
    module.define_singleton_method("native_token_newer", newer)?;
    let raw = function!(fencing::validate_raw::<Family>, 1);
    module.define_singleton_method("native_raw", raw)?;
    let lease = function!(fencing::validate_lease::<Family>, 3);
    module.define_singleton_method("native_lease", lease)?;
    module.define_singleton_method("native_cadence", function!(values::validate_cadence, 3))?;
    let checkpoint = function!(fencing::validate_checkpoint::<Family>, 4);
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
    class.define_method("incumbent", method!(LeaseAuthority::incumbent, 2))?;
    class.define_private_method(
        "native_with_clock_policy",
        method!(LeaseAuthority::with_clock_policy, 1),
    )?;
    class.define_private_method("native_checkpoint", method!(LeaseAuthority::checkpoint, 0))?;
    class.define_private_method("native_acquire", method!(LeaseAuthority::acquire, 3))?;
    class.define_private_method("native_reacquire", method!(LeaseAuthority::reacquire, 3))?;
    class.define_private_method("native_renew", method!(LeaseAuthority::renew, 4))?;
    class.define_private_method("native_release", method!(LeaseAuthority::release, 3))?;
    // Every wrapped class is fixed now, before a method can answer one: no
    // conversion runs `Object.const_get` later.
    Class::pin_wrapped::<LeaseAuthority>(ruby, class)?;

    let class = module.define_class("Fence", ruby.class_object())?;
    class.undef_default_alloc_func();
    class.define_singleton_method("native_with_scope", function!(Fence::with_scope, 1))?;
    class.define_singleton_method("native_resumed", function!(Fence::resumed, 2))?;
    class.define_method("scope", method!(Fence::scope, 0))?;
    class.define_private_method("native_highest", method!(Fence::highest, 0))?;
    class.define_private_method("native_accept", method!(Fence::accept, 1))?;
    Class::pin_wrapped::<Fence>(ruby, class)?;

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
    Class::pin_wrapped::<Holder>(ruby, class).map(drop)
}

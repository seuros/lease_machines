//! Values read from Ruby and built for it.
//!
//! Tokens, leases and checkpoints are read and built by
//! `matryona_machines::ruby::fencing` over [`Family`]: this gem's own class
//! read from the struct, a sibling gem's value of the same shape read by its
//! public readers, a subclass refused, each field converted and the value
//! rebuilt through the core constructor, so a forged value raises before it
//! reaches the core. A cadence is this gem's own only.
use lease_machines::{Cadence, Checkpoint, FencingToken, Lease, OwnerId};
use magnus::{Error, Ruby, Value};
use matryona_machines::ruby::{exact_fields, instant};

use crate::classes::classes;
use crate::holder;

matryona_machines::fencing_family! {
    /// The lease core, for `matryona_machines::ruby::fencing`.
    pub Family in classes {
        token: FencingToken,
        lease: Lease, owner: OwnerId,
        checkpoint: Checkpoint,
    }
}

/// A cadence as the core's: only this gem's `Cadence`.
pub fn cadence(ruby: &Ruby, value: Value) -> Result<Cadence, Error> {
    let class = classes(ruby)?.cadence.get(ruby);
    let message = "expected a LeaseMachines::Cadence";
    let [ttl, renew_after, warn_before] = exact_fields::<3>(ruby, value, class, message)?;
    timing(ruby, ttl, renew_after, warn_before)
}

/// `Cadence::new` of three spans, converted in order.
fn timing(
    ruby: &Ruby,
    ttl: Value,
    renew_after: Value,
    warn_before: Value,
) -> Result<Cadence, Error> {
    let ttl = instant(ttl, "ttl")?;
    let renew_after = instant(renew_after, "renew_after")?;
    holder::cadence(ruby, ttl, renew_after, instant(warn_before, "warn_before")?)
}

/// `LeaseMachines.native_cadence`: what `Cadence.new` validates.
pub fn validate_cadence(
    ruby: &Ruby,
    ttl: Value,
    renew_after: Value,
    warn_before: Value,
) -> Result<(), Error> {
    timing(ruby, ttl, renew_after, warn_before).map(drop)
}

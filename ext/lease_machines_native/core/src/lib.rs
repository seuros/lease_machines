#![no_std]
#![doc = include_str!("../README.md")]

mod authority;
mod fencing;
mod holder;
mod lease;

pub use authority::{Checkpoint, ClockPolicy, Decision, LeaseAuthority, Rejection};
pub use fencing::{Fence, FencingToken};
pub use holder::{Cadence, Holder, HolderState};
pub use lease::{Lease, OwnerId};

#![no_std]
#![doc = include_str!("../README.md")]

mod authority;
mod checkpoint;
mod fencing;
mod holder;
mod lease;
mod refusal;

pub use authority::{ClockPolicy, LeaseAuthority};
pub use checkpoint::Checkpoint;
pub use fencing::{Accepting, Fence, FencingToken};
pub use holder::{Cadence, Holder, HolderState, Recording, Refreshing};
pub use lease::{Lease, OwnerId};
pub use refusal::Refusal;

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;

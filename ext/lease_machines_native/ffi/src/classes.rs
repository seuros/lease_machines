//! The `Data` classes, resolved once at load and pinned. The exact-class
//! check, the own-or-sibling read (see `values`) and clone-then-keep are
//! `matryona_machines::ruby`'s.
use magnus::{Error, RModule, Ruby};
use matryona_machines::ruby::{Class, Registry};

/// Every `Data` class the binding checks or builds.
pub struct Classes {
    pub token: Class,
    pub lease: Class,
    pub checkpoint: Class,
    pub cadence: Class,
}

static CLASSES: Registry<Classes> = Registry::new("lease_machines");

/// Resolve the classes `lease_machines.rb` defined before requiring the
/// extension. Called once, from init; never by name again.
pub fn resolve(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = |name| Class::resolve(ruby, module, name);
    let classes = Classes {
        token: class("FencingToken")?,
        lease: class("Lease")?,
        checkpoint: class("Checkpoint")?,
        cadence: class("Cadence")?,
    };
    CLASSES.set(ruby, classes).map(drop)
}

/// The classes resolved at load.
pub fn classes(ruby: &Ruby) -> Result<&'static Classes, Error> {
    CLASSES.get(ruby)
}

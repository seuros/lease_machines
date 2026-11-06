use core::num::NonZeroU64;

/// A positive fencing sequence number within one resource's authority namespace.
///
/// This value is not a cryptographic proof that an authority issued it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FencingToken(NonZeroU64);

impl FencingToken {
    /// Decode a positive token; zero is not a valid issued token.
    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Sequence number for trusted storage or transport adapters.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0.get()
    }

    pub(crate) fn next(previous: Option<Self>) -> Option<Self> {
        let value = previous.map_or(0, Self::value).checked_add(1)?;
        Self::new(value)
    }
}

/// Protected-resource high-water tracking for a single token namespace.
///
/// Persist updates atomically with the protected write. This local predicate does
/// not check expiry, authenticate tokens, or serialize external mutations.
#[derive(Debug, PartialEq, Eq)]
pub struct Fence {
    highest: Option<FencingToken>,
}

impl Fence {
    /// Restore the newest committed high-water mark, or start a fresh resource.
    #[must_use]
    pub const fn new(highest: Option<FencingToken>) -> Self {
        Self { highest }
    }

    /// Accept an equal/newer token and advance the local high-water mark.
    ///
    /// Returns `false` for a stale token, without changing state.
    #[must_use]
    pub fn accept(&mut self, token: FencingToken) -> bool {
        if self.highest.is_some_and(|highest| token < highest) {
            return false;
        }
        self.highest = Some(token);
        true
    }

    /// Highest accepted token, to persist atomically with protected mutations.
    #[must_use]
    pub const fn highest(&self) -> Option<FencingToken> {
        self.highest
    }
}

#[cfg(test)]
mod tests;

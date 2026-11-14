use core::{cmp::Ordering, num::NonZeroU64};

use periscope_machines::{scope_of, sequence_of};

use crate::Refusal;

/// A fencing token: the issuing authority's scope above a sequence number
/// counted from one.
///
/// Tokens of one scope are totally ordered and the newer one wins. Tokens of
/// different scopes have no order at all, which is why this type implements
/// neither `Ord` nor `PartialOrd`: compare with [`FencingToken::order`] or
/// [`FencingToken::is_newer_than`], which never rank raw numbers across
/// scopes.
///
/// Only a [`LeaseAuthority`](crate::LeaseAuthority) issues tokens. A token
/// rebuilt with [`FencingToken::from_raw`] proves nothing by itself: it is not
/// a cryptographic proof that any authority issued it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FencingToken(NonZeroU64);

// The niche keeps an optional token (a fence's mark, a checkpoint's lease)
// the size of a bare one.
const _: () = assert!(size_of::<Option<FencingToken>>() == size_of::<u64>());

impl FencingToken {
    /// Rebuild a token that crossed a wire or came out of storage.
    ///
    /// `None` when the sequence number is zero, which no authority issues.
    /// Anything else is accepted: this proves nothing about who issued it.
    /// The authority still refuses a token it never issued, a fence one of
    /// another scope.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Option<Self> {
        // A zero raw has sequence zero too, so both arms are real answers.
        match NonZeroU64::new(raw) {
            Some(raw) if sequence_of(raw.get()) != 0 => Some(Self(raw)),
            Some(_) | None => None,
        }
    }

    /// A token the authority's issuer just issued. The issuer counts
    /// sequences from one, so `raw` is never zero and the saturating
    /// arithmetic is the identity on it: no arm for a zero to fall into.
    pub(crate) const fn issued(raw: u64) -> Self {
        debug_assert!(
            sequence_of(raw) != 0,
            "the issuer counts sequences from one"
        );
        Self(NonZeroU64::MIN.saturating_add(raw.saturating_sub(1)))
    }

    /// The whole token for storage or transport: scope in the top 16 bits,
    /// sequence number in the low 48. In scope zero the tokens read 1, 2, 3.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0.get()
    }

    /// The scope of the authority that issued this token.
    #[must_use]
    pub const fn scope(self) -> u16 {
        scope_of(self.raw())
    }

    /// How this token ranks against `other` in their shared scope; `None`
    /// when the scopes differ, because such tokens are not comparable.
    #[must_use]
    pub const fn order(self, other: Self) -> Option<Ordering> {
        if self.scope() != other.scope() {
            return None;
        }
        Some(if self.raw() < other.raw() {
            Ordering::Less
        } else if self.raw() > other.raw() {
            Ordering::Greater
        } else {
            Ordering::Equal
        })
    }

    /// Whether this token is of `other`'s scope and strictly newer.
    ///
    /// False across scopes, so "accept only when newer" fails closed. Its
    /// negation does not mean "older or equal": use [`FencingToken::order`]
    /// when the two cases must be told apart.
    #[must_use]
    pub const fn is_newer_than(self, other: Self) -> bool {
        matches!(self.order(other), Some(Ordering::Greater))
    }
}

/// Honoured outcome of [`Fence::accept`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accepting {
    /// The token is newer than every one accepted before (or the first): the
    /// fence's high-water mark is now this token. Persist it.
    Advanced,
    /// The token equals the high-water mark: another operation under the
    /// lease already in force. Nothing changed.
    Current,
}

/// Protected-resource high-water tracking for the tokens of one scope.
///
/// A fence is built for a scope and never learns it from a token: trusting the
/// first token seen would let any stray token of another namespace capture
/// the resource. Persist updates atomically with the protected write. This
/// local predicate does not check expiry, authenticate tokens, or serialize
/// external mutations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fence {
    scope: u16,
    highest: Option<FencingToken>,
}

impl Default for Fence {
    fn default() -> Self {
        Self::new()
    }
}

impl Fence {
    /// Start a fresh resource that has accepted nothing, for scope zero.
    #[must_use]
    pub const fn new() -> Self {
        Self::with_scope(0)
    }

    /// Start a fresh resource that has accepted nothing, for the tokens of
    /// `scope`: the scope of the authority that guards this resource.
    #[must_use]
    pub const fn with_scope(scope: u16) -> Self {
        Self {
            scope,
            highest: None,
        }
    }

    /// Restore the newest committed high-water mark of `scope`.
    ///
    /// The scope is the caller's, never read out of the saved token: `None`
    /// when `highest` carries another scope. Restoring an older mark than the
    /// newest committed one lets stale tokens back in.
    #[must_use]
    pub const fn resumed(scope: u16, highest: Option<FencingToken>) -> Option<Self> {
        if let Some(token) = highest
            && token.scope() != scope
        {
            return None;
        }
        Some(Self { scope, highest })
    }

    /// The scope whose tokens this fence ranks; every other scope is refused.
    #[must_use]
    pub const fn scope(&self) -> u16 {
        self.scope
    }

    /// Highest accepted token, to persist atomically with protected mutations.
    #[must_use]
    pub const fn highest(&self) -> Option<FencingToken> {
        self.highest
    }

    /// Accept an equal or newer token of this fence's scope.
    ///
    /// # Errors
    /// `Unknown` for a token of another scope, whatever its number;
    /// `StaleToken` for one below the high-water mark. Nothing changes.
    pub const fn accept(&mut self, token: FencingToken) -> Result<Accepting, Refusal> {
        if token.scope() != self.scope {
            return Err(Refusal::Unknown);
        }
        if let Some(highest) = self.highest {
            // Same scope on both sides, so the raw numbers rank them.
            if token.raw() < highest.raw() {
                return Err(Refusal::StaleToken);
            }
            if token.raw() == highest.raw() {
                return Ok(Accepting::Current);
            }
        }
        self.highest = Some(token);
        Ok(Accepting::Advanced)
    }
}

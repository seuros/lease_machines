//! The one answer for every call that was not honoured.
use core::fmt;

/// Why a call was not honoured. Every refusal leaves the authority, fence or
/// holder exactly as it was: no grant, no token, no observed clock moves.
///
/// An idempotent repeat is not a refusal: see [`Recording::Duplicate`](crate::Recording)
/// and [`Accepting::Current`](crate::Accepting).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Another owner's unexpired grant occupies the resource.
    Occupied,
    /// The requesting owner already holds the unexpired grant. `acquire`
    /// cannot prove the call is a retry of the request that was granted, and
    /// answering with the live token would let two incarnations of one owner
    /// share it: `renew` with the token, or `reacquire` to rotate it.
    Held,
    /// No grant exists to renew or release.
    NoLease,
    /// The request's owner is not the current grant's.
    WrongOwner,
    /// The token is of this namespace but older than the one in force: not the
    /// current grant's (authority), below the highest accepted (fence), below
    /// the one held or not strictly newer after expiry (holder).
    StaleToken,
    /// The token is not of this namespace: it carries another scope, or the
    /// authority never issued its sequence number. Refused before any lookup.
    Unknown,
    /// The grant is expired at the instant the call was evaluated at
    /// (authority), or the holder recorded the authority's report of it.
    ///
    /// From the authority it is an answer about that instant, not a recorded
    /// fact: like every refusal it moves no watermark, so the same call at an
    /// earlier instant, not behind the latest honoured one, may be honoured.
    /// Exclusion rests on honoured calls. Another owner gets the grant only
    /// through an honoured `acquire`, which records its instant and issues a
    /// newer token.
    Expired,
    /// `now` is earlier than the latest honoured observation.
    ClockRegressed,
    /// The TTL is zero, or `now + ttl` overflows the duration range.
    InvalidTtl,
    /// Every token of this scope (2^48 - 1) has been issued; tokens never wrap.
    Exhausted,
    /// The holder is fenced; no event leaves that state.
    Fenced,
    /// The holder is not waiting for this report and never recorded it: a
    /// confirmation of a newer token with no renewal outstanding, or a fresh
    /// grant while the current one is not expired.
    OutOfPhase,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Occupied => "another owner's unexpired grant occupies the resource",
            Self::Held => "the owner already holds the unexpired grant",
            Self::NoLease => "no grant exists",
            Self::WrongOwner => "the owner is not the current grant's",
            Self::StaleToken => "the token is older than the one in force",
            Self::Unknown => "the token is not of this namespace",
            Self::Expired => "the grant has expired",
            Self::ClockRegressed => "time is earlier than the latest honoured observation",
            Self::InvalidTtl => "the ttl is zero or overflows",
            Self::Exhausted => "token space exhausted",
            Self::Fenced => "the holder is fenced",
            Self::OutOfPhase => "the holder neither waits for this report nor recorded it",
        })
    }
}

impl core::error::Error for Refusal {}

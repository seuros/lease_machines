# lease-machines

> **AMS Cheriton**, NCC-4211, *Cheriton*-class authority tender.

Engine-room flight manual: model expiring command keys and resource-side fencing.

A deterministic **single-authority decision model**, not a distributed lock,
failure detector, consensus protocol, or storage backend.

## Lease Lifecycle

```rust
use core::time::Duration;
use lease_machines::{Decision, LeaseAuthority, OwnerId, Rejection};

let mut authority = LeaseAuthority::new();
let first = authority.acquire(
    OwnerId::new(1), Duration::ZERO, Duration::from_secs(5),
);
let Decision::Granted(lease) = first else {
    panic!("a fresh authority should grant");
};
assert_eq!(
    authority.acquire(OwnerId::new(2), Duration::from_secs(1), Duration::from_secs(5)),
    Decision::Rejected(Rejection::Occupied),
);
assert_eq!(
    authority.release(OwnerId::new(1), lease.token(), Duration::from_secs(2)),
    Decision::Released,
);
```

- Each authority instance models **one resource and one token namespace**.
- Every acquisition, even by the same owner after release/expiry, gets a strictly
  greater token. Renewal retains the token and never shortens its expiration.
- A lease expires at `now >= expires_at`. Renewal cannot resurrect an expired
  lease. Release and renewal require both the current owner and current token.
- Supply durations from one monotonic origin. Regressing observations are rejected.
  Non-regressing observations advance the clock even when the request is rejected.
- Zero TTL and deadline overflow are rejected. Exhausted token space fails closed;
  tokens never wrap.
- `reacquire` is `acquire` plus one exception: the current owner may rotate its own
  unexpired grant to a new token. The old token is stale from then on, so anything
  still holding it is fenced. Another owner's live grant is still `Occupied`.

## Rebuilding an Authority from Storage

```rust
use core::time::Duration;
use lease_machines::{Checkpoint, Decision, FencingToken, Lease, LeaseAuthority, OwnerId};

# fn example() -> Option<()> {
// One row per resource: owner id, token, expires_at (ms), token high-water.
let (owner, token, expires_at_ms, last_issued) = (7_u64, 3_u64, 9_000_u64, 3_u64);
let lease = Lease::new(
    OwnerId::new(owner), FencingToken::new(token)?, Duration::from_millis(expires_at_ms),
);
let committed = Checkpoint::from_parts(FencingToken::new(last_issued), Some(lease), Duration::ZERO)?;
let mut authority = LeaseAuthority::restore(committed);
let Decision::Granted(next) = authority.acquire(
    OwnerId::new(8), Duration::from_millis(9_000), Duration::from_secs(5),
) else {
    panic!("the previous grant expired");
};
assert_eq!(next.token().value(), 4);
# Some(())
# }
# assert_eq!(example(), Some(()));
```

- `OwnerId` and `FencingToken` are plain `u64` on the wire: `new()` in, `value()` out.
  String owners hash to a `u64` on the caller's side; the authority only compares.
- `from_parts` refuses a lease whose token the history never issued. Keep the
  high-water column even after release; it is what makes the next token greater.
- `observed` is the clock watermark. Pass `Duration::ZERO` when storage keeps none,
  and the authority trusts whatever `now` the next request brings.

## Clocks

Time is a `Duration` from an origin the caller picks: a monotonic start, or the Unix
epoch when the clock is a database's wall clock in milliseconds. One origin per
authority history.

By default an observation earlier than the last one is `Rejection::ClockRegressed`
and nothing changes. `with_clock_policy(ClockPolicy::Clamp)` evaluates such a request
at the last observation instead: expiry checks and new deadlines use the later time,
so a backwards step can never revive a lease or shorten one.

## Fencing at the Protected Resource

```rust
use lease_machines::{Fence, FencingToken};

# fn example() -> Option<()> {
let old = FencingToken::new(7)?;
let current = FencingToken::new(8)?;
let mut fence = Fence::new(Some(old));
assert!(fence.accept(current));
assert!(!fence.accept(old));
assert!(fence.accept(current)); // Multiple operations in one lease are allowed.
# Some(())
# }
# assert_eq!(example(), Some(()));
```

Fencing rejects tokens lower than the highest token the resource has accepted.
Equal tokens remain valid for repeated operations in the same lease.
It does **not** prove current lease validity, authorize an owner, authenticate a
token, or deduplicate operations. A resource cannot reject a newly stale owner on
fencing grounds until it has observed a greater token. If immediate revocation is
required, add an authority/expiry check under an appropriate protocol.

## Holding a Lease

```rust
use core::time::Duration;
use lease_machines::{Cadence, FencingToken, Holder, HolderState};

# fn example() -> Option<()> {
let secs = Duration::from_secs;
let cadence = Cadence::new(secs(30), secs(10), secs(5))?;
let mut holder = Holder::new(FencingToken::new(1)?, secs(0), cadence);
assert!(!holder.renewal_due(secs(9)));
assert!(holder.renewal_due(secs(10)));
assert!(holder.refresh(secs(10)));
assert_eq!(holder.state(), HolderState::Unconfirmed);
assert!(holder.confirm(FencingToken::new(1)?, secs(11)));
assert_eq!(holder.state(), HolderState::Confirmed);
assert!(holder.at_risk(secs(36)));
assert!(holder.expire());
assert!(holder.reacquire(FencingToken::new(2)?, secs(40)));
holder.fence();
assert_eq!(holder.warning_deadline(), None);
# Some(())
# }
# assert_eq!(example(), Some(()));
```

The holder is the other half of the conversation: it decides when to renew, how
nervous to be, and when to stop writing. It runs on the holder's own clock and
never compares instants with the authority.

- `Confirmed` -> `refresh` -> `Unconfirmed`, or `AtRisk` once past the warning
  deadline (`ttl - warn_before` after the last confirmation).
- `confirm(token, at)` closes an attempt and reschedules. A response with an older
  token than the current one is a stale reply and is ignored; the same or a newer
  token is accepted.
- `expire` means the authority said so; only `reacquire` with a strictly newer token
  recovers. `fence` is terminal and idempotent; every predicate answers accordingly.
- `renewal_due` is true whenever a renewal is outstanding, so a failed attempt
  retries on the next poll without any extra state.

## Required Runtime and Persistence Contract

1. There must be one serialized authoritative history per resource. A local
   mutable instance is not enough to arbitrate across partitioned servers.
2. Commit authority state, including token high-water mark, active lease, and
   releases/renewals, **before exposing successful decisions**. On uncertain
   persistence, stop serving and recover; do not continue from ambiguous state.
3. Resource adapters must authenticate issued tokens and atomically persist
   their fencing high-water mark **with the protected mutation**. A separate
   `accept()` call followed by an unguarded write is not safe under concurrency.
4. Restore only the newest committed authority checkpoint, with the same clock
   origin. Never roll back token state or start `new()` over an existing namespace.
   A reboot or clock-origin change requires an external recovery protocol;
   replaying old monotonic durations is not sufficient.
5. Fence state must also survive resource restarts. Token comparisons are only
   meaningful within the same resource namespace.

`checkpoint()` / `restore()` provide an opaque typed state transfer for a host
or deterministic simulation. There is no wire format, storage adapter, distributed
replication, or crash-safe persistence implementation in this initial version.

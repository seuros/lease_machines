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

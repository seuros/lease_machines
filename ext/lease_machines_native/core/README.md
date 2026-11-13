# lease-machines

> **AMS Cheriton**, NCC-4211, *Cheriton*-class authority tender.

Engine-room flight manual: model expiring command keys and resource-side fencing.

A deterministic **single-authority decision model**, not a distributed lock,
failure detector, consensus protocol, or storage backend.

## Lease Lifecycle

```rust
use core::time::Duration;
use lease_machines::{LeaseAuthority, OwnerId, Refusal};

let mut authority = LeaseAuthority::with_scope(7);
let lease = authority
    .acquire(OwnerId::new(1), Duration::ZERO, Duration::from_secs(5))
    .expect("a fresh authority grants");
assert_eq!(lease.token().scope(), 7);
assert_eq!(
    authority.acquire(OwnerId::new(2), Duration::from_secs(1), Duration::from_secs(5)),
    Err(Refusal::Occupied),
);
assert_eq!(
    authority.release(OwnerId::new(1), lease.token(), Duration::from_secs(2)),
    Ok(lease),
);
```

- Each authority instance models **one resource and one token scope**.
- `acquire`, `reacquire`, `renew` and `release` answer `Result<Lease, Refusal>`:
  the grant now in force (for `release`, the grant that was released), or the
  one reason the call was not honoured.
- Every acquisition, even by the same owner after release or expiry, gets a
  strictly newer token. Renewal retains the token and never shortens its expiration.
- A lease is expired at any `now >= expires_at`, and a renewal or release
  evaluated at such an instant is refused. That answer is about the instant
  supplied and is not recorded (see [Time](#time)). Release and renewal require
  both the current owner and current token.
- Zero TTL and deadline overflow are refused. An exhausted scope (2^48 - 1
  tokens) fails closed; tokens never wrap.
- `reacquire` is `acquire` plus one exception: the current owner may rotate its own
  unexpired grant to a new token. The old token is stale from then on, so anything
  still holding it is fenced. Another owner's live grant is still `Occupied`.

## Contract

### Refusals

`Refusal` is the crate's only refusal type, and **a refusal changes nothing**:
no grant, no token, no observed clock. Every operation that can refuse returns
`Result<T, Refusal>` and `T` describes only honoured outcomes.

| Refusal | Meaning | Answered by |
| --- | --- | --- |
| `Occupied` | another owner's unexpired grant | `acquire`, `reacquire` |
| `Held` | the owner's own unexpired grant | `acquire` |
| `NoLease` | nothing to renew or release | `renew`, `release` |
| `WrongOwner` | the owner is not the grant's | `renew`, `release` |
| `StaleToken` | a token of this scope, older than the one in force | `renew`, `release`, `Fence::accept`, `Holder::confirm`, `Holder::reacquire` |
| `Unknown` | a token of another scope, or one the authority never issued | `renew`, `release`, `Fence::accept`, `Holder::confirm`, `Holder::reacquire` |
| `Expired` | the grant is expired at the instant supplied (authority); the holder recorded `expire` (holder) | `renew`, `release`, `Holder::confirm` |
| `ClockRegressed` | `now` is before the latest honoured observation | every authority call, under `ClockPolicy::Reject` |
| `InvalidTtl` | zero TTL, or `now + ttl` overflows | `acquire`, `reacquire`, `renew` |
| `Exhausted` | the scope's tokens are spent | `acquire`, `reacquire` |
| `Fenced` | the holder is fenced | `Holder::refresh`, `confirm`, `expire`, `reacquire` |
| `OutOfPhase` | the holder neither waits for the report nor recorded it | `Holder::confirm`, `Holder::reacquire` |

The authority checks in this order: `Unknown`, `ClockRegressed`, `NoLease`,
`WrongOwner`, `StaleToken`, `Expired`, `InvalidTtl` for `renew` and `release`;
`ClockRegressed`, `InvalidTtl`, `Occupied`, `Held`, `Exhausted` for `acquire`
and `reacquire`. Issuing the token is the last step, so a call refused for any
reason consumes none.

### Repeats

A repeat is honoured only when the core can prove that what it asks for
already holds.

- `Holder::confirm` with the token already held, while confirmed, is
  `Ok(Recording::Duplicate)`; so is `Holder::reacquire` with it, and
  `Holder::expire` on an expired holder. Nothing changes, the renewal schedule
  included: the instant a report arrived is not part of the report.
- `Fence::accept` of the token at the high-water mark is `Ok(Accepting::Current)`;
  a newer one is `Ok(Accepting::Advanced)`.
- `renew` again is simply honoured: it never shortens, so a replay loses nothing.
- `acquire` by the owner of the live grant is **not** a repeat. The grant
  records neither the TTL asked for nor when it was asked, and the caller
  presents no token, so the core cannot tell a retry from a second incarnation
  of that owner. Answering with the live token would let both write under it.
  It is `Refusal::Held`: `renew` with the token, or `reacquire` to rotate it.
- `release` again is `Refusal::NoLease`. The authority keeps no record of who
  released what, so it cannot prove the caller's claim.
- `reacquire` again rotates again: every honoured call issues a new token.

### Scopes and tokens

A `FencingToken` is the issuing authority's scope in the top 16 bits above a
sequence number counted from one (`periscope-machines`). `LeaseAuthority::new()`
is scope zero, where tokens read 1, 2, 3; `with_scope(scope)` stamps every token.

- Tokens of one scope are totally ordered and the newer one wins. Tokens of
  two scopes have no order, so `FencingToken` implements neither `Ord` nor
  `PartialOrd`. `order` answers `None` across scopes and `is_newer_than`
  answers `false`, which makes "accept only when newer" fail closed.
- The authority refuses a token it never issued (`Unknown`) before any lookup:
  another scope, or a sequence number beyond the last one issued.
- A `Fence` is built for a scope (`new()` is scope zero, `with_scope`,
  `resumed(scope, highest)`) and refuses every token of another, whatever its
  number. It never learns its scope from the first token it sees: that would
  let a stray token of another namespace capture the resource.
- A `Holder` follows the tokens of the scope it was built with.
- `FencingToken::from_raw(raw)` rebuilds a token that crossed a wire and `raw()`
  takes it apart. A rebuilt token proves nothing by itself: it is not a
  cryptographic proof that any authority issued it. Authenticate it in the adapter.
- Two authorities left in one scope still issue identical tokens. Keeping
  scopes distinct is the caller's job.

### Time

Every instant and span is a `core::time::Duration` from an origin the caller
picks: a monotonic start, or the Unix epoch when the clock is a database's wall
clock. One origin per authority history. The holder runs on its own clock and
never compares instants with the authority.

The authority's watermark (`Checkpoint::observed`) is the latest instant of an
**honoured** call. A refused call is checked against it and never moves it.

- `ClockPolicy::Reject` (the default): a `now` before the watermark is
  `Refusal::ClockRegressed`.
- `ClockPolicy::Clamp`: such a call is evaluated at the watermark instead. A
  backwards step therefore cannot undo what an honoured call did: it cannot
  bring back a grant that was released or replaced, and it cannot shorten one.
  If the clamped call is honoured the watermark stays where it was; if it is
  refused for another reason nothing changes, like any refusal.

**`Expired` is an answer about the instant supplied, not a recorded fact.**
Under either policy a refused `Expired` moves no watermark, so the same call
at an earlier instant that is not behind the latest honoured one may be
honoured:

```rust
use core::time::Duration;
use lease_machines::{Lease, LeaseAuthority, OwnerId, Refusal};

let secs = Duration::from_secs;
let (a, b) = (OwnerId::new(1), OwnerId::new(2));
let mut authority = LeaseAuthority::new();
let token = authority.acquire(a, secs(0), secs(10)).expect("vacant").token();
assert_eq!(authority.renew(a, token, secs(15), secs(10)), Err(Refusal::Expired));
// Nothing was recorded at 15s; 5s is not behind the watermark (0s).
assert_eq!(authority.renew(a, token, secs(5), secs(10)), Ok(Lease::new(a, token, secs(15))));
// The slot was `a`'s throughout, and `b` enters only through an honoured
// `acquire`, which is remembered and takes a newer token.
assert_eq!(authority.acquire(b, secs(12), secs(10)), Err(Refusal::Occupied));
assert_eq!(authority.acquire(b, secs(15), secs(10)).map(|l| l.token().raw()), Ok(2));
assert_eq!(authority.renew(a, token, secs(16), secs(10)), Err(Refusal::WrongOwner));
```

Nothing may rest on a refused instant being final. What does hold:

- There is one grant slot, so at most one grant exists at any point of the
  history.
- Another owner gets the slot only through an honoured `acquire` or
  `reacquire`. That call records its instant as the watermark and issues a
  strictly newer token, so the previous owner is `WrongOwner` from then on
  and its token is stale at any fence that saw the new one.
- A `Holder` that was told `Expired` and recorded it with `expire` answers a
  late confirmation `Expired`; only `reacquire` with a strictly newer token
  recovers it.

## Rebuilding an Authority from Storage

```rust
use core::time::Duration;
use lease_machines::{Checkpoint, ClockPolicy, FencingToken, Lease, LeaseAuthority, OwnerId};

# fn example() -> Option<()> {
// One row per resource: scope, tokens issued, then the live grant if any.
let (scope, issued) = (7_u16, 3_u64);
let (owner, token, expires_at_ms) = (9_u64, (7_u64 << 48) | 3, 9_000_u64);
let lease = Lease::new(
    OwnerId::new(owner), FencingToken::from_raw(token)?, Duration::from_millis(expires_at_ms),
);
let committed = Checkpoint::from_parts(scope, issued, Some(lease), Duration::ZERO)?;
let mut authority = LeaseAuthority::restored(scope, committed, ClockPolicy::Reject)?;
let next = authority
    .acquire(OwnerId::new(8), Duration::from_millis(9_000), Duration::from_secs(5))
    .ok()?;
assert_eq!(next.token().raw(), (7 << 48) | 4);
# Some(())
# }
# assert_eq!(example(), Some(()));
```

- A `Checkpoint` is four values: `scope`, `issued` (how many tokens the history
  has issued, which is the sequence number of the last), the live `lease` if
  any, and the `observed` watermark. `LeaseAuthority::checkpoint()` takes one;
  `Checkpoint::from_parts` rebuilds one from columns.
- `from_parts` answers `None` when `issued` is beyond the scope's token space,
  or when the lease is not the grant the history ends with: it must carry
  exactly token number `issued` of `scope`. A lease under an older token was
  superseded, and one under another scope was never granted here.
- `restored(scope, checkpoint, clock)` takes the scope from the caller, never
  from the saved value, and answers `None` for a checkpoint of another scope.
  The clock policy is configuration, not committed state, so it is stated again
  rather than silently reset.
- `OwnerId` is the caller's `u64`: `new()` in, `value()` out. String owners hash
  to a `u64` on the caller's side; the authority only compares.
- Pass `Duration::ZERO` as `observed` when storage keeps no watermark, and the
  authority trusts whatever `now` the next request brings.

## Fencing at the Protected Resource

```rust
use lease_machines::{Accepting, Fence, FencingToken, Refusal};

# fn example() -> Option<()> {
let scope = 7_u16;
let token = |sequence: u64| FencingToken::from_raw((u64::from(scope) << 48) | sequence);
let (old, current) = (token(7)?, token(8)?);
let mut fence = Fence::resumed(scope, Some(old))?;
assert_eq!(fence.accept(current), Ok(Accepting::Advanced));
assert_eq!(fence.accept(old), Err(Refusal::StaleToken));
// Multiple operations in one lease are allowed.
assert_eq!(fence.accept(current), Ok(Accepting::Current));
// A bigger number from another scope outranks nothing.
assert_eq!(fence.accept(FencingToken::from_raw((9 << 48) | 1)?), Err(Refusal::Unknown));
# Some(())
# }
# assert_eq!(example(), Some(()));
```

Fencing refuses tokens older than the highest the resource has accepted, and
every token of another scope. Equal tokens remain valid for repeated operations
in the same lease; `Accepting` says whether the mark moved, which is when it
must be persisted. It does **not** prove current lease validity, authorize an
owner, authenticate a token, or deduplicate operations. A resource cannot
refuse a newly stale owner on fencing grounds until it has observed a newer
token. If immediate revocation is required, add an authority/expiry check
under an appropriate protocol.

## Holding a Lease

```rust
use core::time::Duration;
use lease_machines::{Cadence, FencingToken, Holder, HolderState, Recording, Refreshing};

# fn example() -> Option<()> {
let secs = Duration::from_secs;
let cadence = Cadence::new(secs(30), secs(10), secs(5))?;
let mut holder = Holder::new(FencingToken::from_raw(1)?, secs(0), cadence);
assert!(!holder.renewal_due(secs(9)));
assert!(holder.renewal_due(secs(10)));
assert_eq!(holder.refresh(secs(10)), Ok(Refreshing::Started));
assert_eq!(holder.state(), HolderState::Unconfirmed);
assert_eq!(holder.confirm(FencingToken::from_raw(1)?, secs(11)), Ok(Recording::Recorded));
assert_eq!(holder.confirm(FencingToken::from_raw(1)?, secs(12)), Ok(Recording::Duplicate));
assert!(holder.at_risk(secs(36)));
assert_eq!(holder.expire(), Ok(Recording::Recorded));
assert_eq!(holder.reacquire(FencingToken::from_raw(2)?, secs(40)), Ok(Recording::Recorded));
holder.fence();
assert_eq!(holder.warning_deadline(), None);
# Some(())
# }
# assert_eq!(example(), Some(()));
```

The holder is the other half of the conversation: it decides when to renew, how
nervous to be, and when to stop writing. It runs on the holder's own clock and
never compares instants with the authority.

- `refresh(now)`: `Started` when an attempt begins (`Confirmed` becomes
  `Unconfirmed`, or `AtRisk` once past the warning deadline, `ttl - warn_before`
  after the last confirmation); `Outstanding` when one already was.
- `confirm(token, at)` closes an attempt and reschedules. An older token is a
  late reply (`StaleToken`); the same or a newer token of the holder's scope is
  recorded. With no attempt outstanding, the token already held is a
  `Duplicate` and a newer one is `OutOfPhase`.
- `expire` means the authority said so; only `reacquire` with a strictly newer
  token recovers. `fence` is terminal, cannot refuse, and every later report is
  `Fenced`.
- `renewal_due` is true whenever a renewal is outstanding, so a failed attempt
  retries on the next poll without any extra state.

## Required Runtime and Persistence Contract

1. There must be one serialized authoritative history per resource. A local
   mutable instance is not enough to arbitrate across partitioned servers.
2. Commit the checkpoint (scope, tokens issued, live lease, watermark)
   **before exposing successful decisions**. On uncertain persistence, stop
   serving and recover; do not continue from ambiguous state.
3. Resource adapters must authenticate issued tokens and atomically persist
   their fencing high-water mark **with the protected mutation**. A separate
   `accept()` call followed by an unguarded write is not safe under concurrency.
4. **Restarts.** A fresh authority counts from one again, so it reissues every
   token its predecessor issued. Either restore the newest committed
   checkpoint with `restored`, under the same scope and clock origin, or start
   the new incarnation in a scope no earlier one used, and rebuild every fence
   for that scope out of band. Restoring an older checkpoint is a second
   authority in the same scope: it reissues tokens, and no pure value can
   detect it. `Checkpoint` is not `Copy` for that reason, and that is all the
   help the type system can give.
5. Fence state must also survive resource restarts: `Fence::resumed(scope,
   highest)`. Token comparisons are only meaningful within one scope.

There is no wire format, storage adapter, distributed replication, or
crash-safe persistence implementation here. The checkpoint's four readers and
`FencingToken::raw` are the columns; the encoding is the adapter's.

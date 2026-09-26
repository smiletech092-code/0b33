# `batch_charge`

Charges multiple subscriptions in one transaction. The entrypoint is
admin-only and is guarded by the emergency stop flag.

## Signature
```rust
pub fn batch_charge(
	env: Env,
	subscription_ids: Vec<u32>,
	nonce: u64,
) -> Result<Vec<BatchChargeResult>, Error>
```

## Maximum batch size

**`BATCH_MAX_SIZE` = 100 ids per call.** The bound is a compile-time constant
(`types::BATCH_MAX_SIZE`, re-exported at the crate root as
`subscription_vault::BATCH_MAX_SIZE`) and is enforced in `do_batch_charge`
before any id is processed.

| `subscription_ids.len()` | Behaviour |
|--------------------------|-----------|
| `0` | No-op. Returns an empty vector; the nonce is **not** consumed. |
| `1 ..= 100` | Processed normally (subject to the duplicate-id rule below). |
| `> 100` | Rejected wholesale with `Error::InvalidInput` (code `3002`). No id is charged, no result vector is returned, and the nonce is **not** consumed. |

### Why the cap exists

`batch_charge` charges ids **sequentially inside one transaction**. Each id
costs a bounded but non-trivial number of instructions (subscription read, fee
maths, token transfer, storage writes, event emission). Soroban's
per-transaction instruction budget is a *network* limit, not a contract limit,
so without a contract-level cap an oversized batch does not fail with a
contract error — it aborts with a generic execution error that a caller cannot
distinguish from genuine out-of-gas and cannot handle programmatically.

The cap converts that ambiguous, unrecoverable failure into a deterministic,
catchable `InvalidInput` **before** any state is touched, so the caller can
split the batch and retry with the same nonce.

### Sizing guidance

- **Hard ceiling:** 100 ids.
- **Recommended working size:** ~50 ids or fewer. Per-item cost is not uniform
  — a cold subscription read is more expensive than a warm one — so a batch
  near the ceiling is more likely to approach the network instruction budget.
- **Batching pattern:** chunk a larger billing run into ceil(N / 50) calls of
  at most 50 ids each. Because each call consumes its own nonce, use a
  distinct nonce per chunk.
- Treat 100 as a rejection threshold to stay away from, not a target.

### Oversized-batch retry

Both argument-shape checks — batch length and duplicate ids — run **before**
the nonce is consumed. A rejected batch therefore leaves no trace:

```
attempt 1: batch_charge([...101 ids...], nonce=7)
  → Err(InvalidInput)         // oversized
  // nonce 7 is still unused

attempt 2: batch_charge([...50 ids...], nonce=7)   // chunk 1
  → Ok([...])
attempt 3: batch_charge([...51 ids...], nonce=8)   // chunk 2
  → Ok([...])
```

Reusing `nonce = 7` after an `InvalidInput` rejection is safe and is the
recommended way to recover. In contrast, a batch that *ran* always consumes
its nonce, so a retry of a partially-successful batch needs a fresh one (see
[Retry guidance](#retry-guidance)).

For idempotency-key semantics and why double-charging is prevented, see
[`replay_protection.md`](replay_protection.md).

## Partial-success model

Admin authentication and the batch nonce check happen once at the batch
boundary. After those checks pass, each ID is processed independently through
the shared `charge_one` path. An item failure does not roll back successful
items or abort the remaining items. The call returns exactly one result for
each input ID, including IDs that are missing or repeated.

## `BatchChargeResult`

The result vector has the same order and length as `subscription_ids`:

| Field | Type | Values |
|-------|------|--------|
| `success` | `bool` | `true` when that item completed without an item-level error |
| `error_code` | `u32` | `0` on success; otherwise the corresponding `Error` code |

Successful interval charges mutate only their own subscription and accounting
state. Failed items return their error code and retain the single-charge
semantics, including any per-item lifecycle transition such as entering
`GracePeriod` or `InsufficientBalance`.

## Ordering guarantees

Results are appended while iterating `subscription_ids`, so result `i`
corresponds to input ID `i`. Processing is sequential. This means duplicate
IDs are processed at each position, and a later duplicate observes state
written by the earlier occurrence in the same batch.

## Skip conditions
- Subscription not found
- Status is Paused, Cancelled, or InsufficientBalance
- Billing interval has not elapsed
- Insufficient prepaid balance (also applies the lifecycle grace rule)

## Retry guidance

The batch nonce is consumed before item processing. A retry of the same batch
must therefore use a fresh nonce; reusing the old nonce is rejected as a
replay. Prefer retrying only the failed IDs after correcting their cause:
top up and explicitly resume an underfunded subscription, wait for an interval
that has not elapsed, or remove/replace an ID that no longer exists. Do not
blindly retry successful IDs, because their next result may be
`IntervalNotElapsed` or another state-dependent outcome.

The outer `Result` is an error rather than a result vector when the emergency
stop is active, authentication fails, or the batch nonce is invalid. In
particular, enabling the emergency stop prevents any item from being charged;
it does not produce partial per-item results. Pause, resume, cancel, and query
operations remain available according to the emergency-stop policy.

# Billing Interval Enforcement

How `charge_subscription` validates and enforces timing between charges.

---

## Interval constraints

| Bound | Value | Constant |
|-------|-------|----------|
| Minimum (enforced) | 60 s (1 minute) | `MIN_SUBSCRIPTION_INTERVAL_SECONDS` |
| Maximum (enforced) | 31 536 000 s (365 days) | `MAX_SUBSCRIPTION_INTERVAL_SECONDS` |
| Ledger close time (network) | ~5 s | — |

`interval_seconds = 0` is implicitly rejected because zero is below the minimum.
It is additionally rejected by the dedicated guard
`validation::reject_zero_interval`, which runs first so the zero case always
reports the same error regardless of which bound it would otherwise trip.

Validation is performed by the single authoritative helper `validate_interval(interval_seconds)` at every entry point that persists an interval:

- `create_subscription` / `create_subscription_with_token`
- `create_plan_template` / `create_plan_template_with_token`
- `update_plan_template`

### Why the minimum is 60 seconds, not 5 or 10

Stellar ledgers close roughly every **5 seconds** on Testnet and Futurenet
(the target ~5 s close time is a protocol parameter, so treat it as an
approximation, not a guarantee). That is the number that makes sub-minute
intervals look attractive: a 10-second interval is only two ledger closes.

Two or three ledger closes is *not* a safe billing period, and the reason is
not precision — it is the interaction between interval length, ledger close
scheduling, and the charge guard `now >= last_payment + interval_seconds`:

| Concern | Why a sub-minute interval fails |
|---------|----------------------------------|
| **Clock granularity** | `env.ledger().timestamp()` has one-second resolution but advances in ~5 s steps. A 5 s or 10 s interval lands on the same coarse grid as the ledger itself, so "when is the charge due" becomes indistinguishable from "which ledger am I in". |
| **Boundary flapping** | With an interval close to the close time, a charge that lands one ledger early fails with `IntervalNotElapsed` and the next attempt may be a whole ledger later. Retries then alternate between `IntervalNotElapsed` and success. |
| **Batch amplification** | `batch_charge` processes ids sequentially. Sub-minute intervals make far more subscriptions eligible per billing run, so a single admin transaction can exceed the network instruction budget. See [`batch_charge.md`](batch_charge.md#maximum-batch-size). |
| **Ledger-close backpressure** | A ledger that overruns delays the next close. Under load a nominal 5 s close can stretch well past 5 s, so any interval sized against exactly one close is undersized in practice. |
| **Fee rounding** | Fees are applied per charge. Charging on a short cycle multiplies the number of rounding events, and small per-charge amounts can round toward zero. See [`safe_math.md`](safe_math.md). |

At 60 s the interval spans roughly **12 average ledger closes**, which leaves
substantial headroom against a stretched close, and keeps a subscription's
charge rate to at most once per 12 ledgers. The enforced minimum is therefore
roughly **an order of magnitude above the 1–2 ledger-close floor** — high
enough that a normal close-time overrun cannot make a due charge look
not-yet-eligible.

### Guidance for integrators

| Category | Value | Contract behaviour |
|----------|-------|--------------------|
| **Recommended** | ≥ 3 600 s (1 hour) | Accepted. Normalises ledger-close jitter and keeps per-charge fees above rounding thresholds. |
| **Acceptable** | 60 s – 3 599 s | Accepted. Fine for short-cycle or usage-heavy plans; expect some `IntervalNotElapsed` results on tight retry loops. |
| **Rejected** | 1 s – 59 s (sub-minute) | `Error::InvalidInput` (code `3002`) at creation/update. Even though 5–10 s spans 1–2 ledger closes, the contract refuses it. |
| **Rejected** | `0` | `Error::InvalidInput` (code `3002`). |

⚠ **Sub-minute intervals are rejected outright.** `validate_interval` applies
the bounds before the subscription is ever written, so the rejection happens
at `create_subscription` / `update_plan_template` time — not at charge time.
A plan that needs sub-minute billing must model it as a usage-metered
subscription (see [`usage_billing.md`](usage_billing.md)) charged via
`charge_usage` on demand, rather than as an interval charge.

The minimum is enforced in two layers:

1. `validation::reject_zero_interval` — the authoritative zero guard.
2. `validate_interval` in `subscription.rs` — the range check that calls (1)
   first and then compares against `MIN_SUBSCRIPTION_INTERVAL_SECONDS` and
   `MAX_SUBSCRIPTION_INTERVAL_SECONDS`.

---

## Canonical time-math formula

The **next allowed charge timestamp** is computed by the single helper:

```
next_charge_time(last_payment, interval) = last_payment + interval
```

Implemented via checked addition; returns `Err(Overflow)` if the result
would exceed `u64::MAX`.  In practice this cannot happen for validated
intervals (≤ 365 days) and real Stellar ledger timestamps.

Both the **charge path** (`charge_core.rs`) and the **query path**
(`queries.rs`) import this same function from `subscription.rs` so that
the boundary each enforces or displays is identical.

---

## Charge rule

A charge is allowed when:

```
env.ledger().timestamp() >= last_payment_timestamp + interval_seconds
```

The comparison is **inclusive** — a charge at exactly the boundary succeeds.
The comparison at exactly `last_payment_timestamp + interval_seconds - 1` fails.

### Outcomes

| Condition | Result | Storage |
|-----------|--------|---------|
| `now < last_payment + interval` | `Error::IntervalNotElapsed` | Unchanged |
| `now >= last_payment + interval` | Ok | `last_payment_timestamp = now` |
| Subscription not Active/GracePeriod | `Error::NotActive` | Unchanged |
| Subscription not found | `Error::NotFound` | Unchanged |

---

## Timestamp source

All timing uses the Soroban ledger timestamp (`env.ledger().timestamp()`), a
Unix epoch value in seconds controlled by the Stellar validator network.

---

## Window reset

On success, `last_payment_timestamp` is set to the **current ledger
timestamp**, not `last_payment_timestamp + interval_seconds`.  This means
late charges shift the next window forward rather than allowing a cascade of
back-to-back catch-up charges.

### Example (30-day interval)

```
T0 = creation          → last_payment_timestamp = T0
T0 + 30d               → charge succeeds, last_payment_timestamp = T0 + 30d
T0 + 30d (immediate)   → retry rejected (IntervalNotElapsed)
T0 + 60d               → next charge succeeds
```

---

## First charge

`last_payment_timestamp` is initialised to `env.ledger().timestamp()` at
subscription creation, so the first charge cannot occur until
`interval_seconds` later.

---

## Ledger time monotonicity

Soroban ledger timestamps are set by Stellar validators and are expected to be
**non-decreasing** across ledger closes (~5-6 s on mainnet).  The contract does
**not** assume strict monotonicity — it only checks
`now >= last_payment_timestamp + interval_seconds`.  Consequences:

- If two consecutive ledgers share the same timestamp, a charge that just
  succeeded will simply be rejected on the next call because `0 < interval_seconds`.
- The contract never compares the current timestamp to a "previous ledger
  timestamp"; it only compares against its own stored `last_payment_timestamp`.

---

## Timestamp regression safety

Ledger timestamps may move **backward** in practice during:
- Network reorgs / ledger rewinds in test harnesses.
- Soroban host upgrades that reset validator time.
- Deliberate fuzz/chaos testing.

All timestamp subtraction in this contract uses **saturating arithmetic** to
guarantee safety:

| Expression | Location | Regression behaviour |
|---|---|---|
| `now.saturating_sub(sub.start_time)` | `charge_core.rs` | Saturates to 0 → period 0 |
| `now.saturating_sub(state.last_usage_timestamp)` | `charge_core.rs` | elapsed = 0 → burst limit enforced |
| `now.saturating_sub(sub.start_time) / interval` | `charge_core.rs` | period_index = 0 |
| `grace_start.saturating_add(grace_duration)` | `charge_core.rs`, `queries.rs` | No overflow |
| `last_payment.checked_add(interval)` | `subscription.rs` | Returns `Err(Overflow)` on wrap |

### Backward-jump semantics

A **backward jump** (now < last_payment_timestamp) is treated as *"no time has
passed"*:

- `next_charge_time(last_payment, interval) = last_payment + interval`
- Since `now < last_payment <= last_payment + interval`, the guard `now >= next`
  evaluates to `false` → `Error::IntervalNotElapsed` is returned.
- No storage is mutated; `last_payment_timestamp` is unchanged.
- This is the **correct conservative** behaviour: missing a charge window is
  recoverable (retry later); a spurious charge caused by wrap-around would be
  a funds-loss bug.

---

## Security notes

### Double-charge prevention

The boundary check `now >= last + interval` uses checked addition, so there
is no risk of wrap-around confusion.  The integer division-based replay guard
(`period_index = now / interval`) provides a second, independent barrier
against charging twice within the same period.

### Overflow

`next_charge_time` uses `u64::checked_add` and propagates `Error::Overflow`
rather than silently wrapping.  The maximum interval (365 days) means the
furthest future timestamp that can be stored is
`u64::MAX_REALISTIC_TIMESTAMP + 365 days`, which is far below `u64::MAX`
for any foreseeable ledger.

The query path (`compute_next_charge_info`) calls the same helper and uses
`saturating_add` on the (unreachable) overflow path so that display code always
receives a valid timestamp without panicking.

---

## Test coverage

| Test | Scenario |
|------|----------|
| `test_interval_zero_rejected` | `interval = 0` — creation rejected |
| `test_interval_below_min_rejected` | `interval = MIN - 1` — creation rejected |
| `test_interval_at_min_accepted` | `interval = MIN (60 s)` — creation accepted |
| `test_interval_above_max_rejected` | `interval = MAX + 1` — creation rejected |
| `test_interval_at_max_accepted` | `interval = MAX (365 d)` — creation accepted |
| `test_plan_template_interval_below_min_rejected` | Plan template: `interval < MIN` — rejected |
| `test_plan_template_interval_at_min_accepted` | Plan template: `interval = MIN` — accepted |
| `test_plan_template_interval_above_max_rejected` | Plan template: `interval > MAX` — rejected |
| `test_plan_template_interval_at_max_accepted` | Plan template: `interval = MAX` — accepted |
| `test_charge_at_exact_boundary_succeeds` | `now = last + interval` — charge ok |
| `test_charge_one_second_before_boundary_rejected` | `now = last + interval - 1` — IntervalNotElapsed |
| `test_charge_past_boundary_succeeds` | `now >> last + interval` — charge ok |
| `test_window_resets_to_now_after_charge` | Window reset semantics verified |
| `test_max_interval_boundary` | `interval = MAX`: boundary and just-before verified |
| `test_compute_next_charge_info_max_interval_no_overflow` | MAX interval query: no overflow, correct value |
| `test_next_charge_info_matches_charge_enforcement` | Query timestamp == charge enforcement threshold |
| `test_consecutive_interval_charges_no_drift` | 6 consecutive charges at exact boundaries + trailing rejection |
| **Timestamp chaos tests** (`tests/timestamp_chaos.rs`) | |
| `test_backward_jump_by_one_second` | `now = last_payment - 1` → IntervalNotElapsed, no wrap |
| `test_timestamp_jump_to_u64_max` | `now = u64::MAX` → charge succeeds, last_payment = u64::MAX |
| `test_repeated_identical_timestamps_no_double_charge` | Same ts twice → second call rejected, balance unchanged |
| `test_backward_jump_across_grace_boundary_no_panic` | Backward jump while in GracePeriod → no expiry, no panic |
| `test_chaos_200_random_timestamp_jumps` | 200 random forward/backward jumps → all invariants hold |
| `test_monotonic_forward_sequence_all_charges_succeed` | 6 strictly forward charges → all succeed (no false IntervalNotElapsed) |
| `test_backward_jump_then_forward_recovery` | Backward rejects charge → forward past interval → succeeds |
| `test_u64_max_then_backward_to_zero` | Charge at u64::MAX, jump to 0 → rejected, last_payment unchanged |
| `test_get_next_charge_info_stable_on_chaos_timestamps` | 100 random timestamps → query path never panics |
| `test_period_index_saturates_to_zero_when_now_before_start` | `now < start_time` → period_index = 0, no wrap |


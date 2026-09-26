# Optional oracle pricing

The subscription vault supports optional cross-currency pricing through an external oracle contract.

## Oracle interface

When enabled, the vault calls oracle method:

- `latest_price() -> OraclePrice`

`OraclePrice` fields:

- `price`: quote units per 1 token (must be positive)
- `timestamp`: quote publication time

## Configuration

Admin-only:

- `set_oracle_config(admin, enabled, oracle, max_age_seconds)`

Read:

- `get_oracle_config()`

Safety checks:

- enabled requires oracle address
- enabled requires `max_age_seconds > 0` (zero disables staleness guard and is rejected)
- stale data rejected when quote age exceeds `max_age_seconds`
- zero/negative price rejected
- zero timestamp rejected as unavailable

## Charge conversion

With oracle disabled, `subscription.amount` is treated as token-denominated (existing behavior).

With oracle enabled, `subscription.amount` is interpreted as quote-denominated and converted:

`token_amount = ceil(quote_amount * 10^token_decimals / price)`

This preserves deterministic charging while allowing quote-currency plan pricing.

## Failure modes

- `OracleNotConfigured`
- `OraclePriceUnavailable`
- `OraclePriceStale`
- `OraclePriceInvalid`
- `OracleDeviationTooHigh`
- `InvalidAmount` — the resolved token amount rounded to zero (see below)

These errors cause the charge to fail without mutating balances.

## Degenerate prices

The conversion is a **ceiling** division:

```
token_amount = ceil(quote_amount * 10^token_decimals / price)
             = (quote_amount * 10^token_decimals + price - 1) / price
```

Because it rounds *up*, it can never round a charge **down** to zero. That
makes the behaviour at the ends of the price range asymmetric, and the
`price == 1` case in particular is the opposite of what the name suggests.

### `price == 0` and negative — rejected

`price <= 0` is rejected with `OraclePriceInvalid` before any arithmetic runs,
so the division is never evaluated. This is a hard validity gate, not a
sanity band.

### `price == 1` — **accepted**, and produces the *largest* charge

`1` is a legal `i128` and passes the `price > 0` gate, so it is **not**
rejected. The ceiling adjustment `price - 1` becomes `0` and the division
becomes exact:

```
token_amount = (quote_amount * 10^token_decimals + 0) / 1
             = quote_amount * 10^token_decimals
```

So a `price == 1` response does **not** cause a zero charge. It causes a
charge scaled by the full decimal factor — for a 6-decimal token, one million
times the quote amount. Practically:

- If the subscriber is underfunded, the charge fails with
  `InsufficientBalance` and the subscription enters `GracePeriod` /
  `InsufficientBalance`. Loud, and recoverable.
- If the subscriber happens to be well funded, the charge **succeeds** and
  drains them. This is the dangerous case, and no contract-level arithmetic
  check catches it — `price == 1` is arithmetically consistent.

`price == 1` should therefore be read as *the oracle reporting a broken or
placeholder value*, not as *the asset being worthless*.

### When the resolved amount *is* zero

A zero charge can only come from a zero quote amount, not from a bad price:

```
quote_amount == 0  →  numerator == 0  →  token_amount == 0
```

That case returns `InvalidAmount` (code `3001`), **not** `OraclePriceInvalid`.
This distinction is deliberate: the price is fine, the amount is not, and
reporting a price error would point operators at the wrong subsystem.

### Protection: arm the deviation circuit breaker

The contract's defence against a `price == 1` (or any wild) reading is
`check_deviation_and_record`, which compares each price against the median of
recent history:

| `oracle_deviation_bps` | Behaviour on a `price == 1` reading |
|------------------------|--------------------------------------|
| `None` (not configured) | **No protection.** The price is consumed as-is. |
| `Some(0)` | Strictest. Any deviation from the median — including `price == 1` — returns `OracleDeviationTooHigh`. |
| `Some(n)` | Rejects when `deviation_bps > n`. Reasonable defaults are in "Recommended global values by billing cadence" below. |

⚠ **Bootstrap gap:** while a token has no price history, the first reading is
always accepted regardless of value. A `price == 1` as the *very first*
observation therefore reaches a charge. There is no median to compare against
yet, so this is a property of the design, not a bug — but it means the
breaker protects steady state, not the first observation after
configuration, contract upgrade, or history expiry.

### Operator checklist

1. Set `oracle_deviation_bps` — do not leave it unset. Unset means no
   deviation protection at all.
2. On enabling the oracle, submit a known-good reference price first so the
   history is seeded before any real charge can run against a bootstrap read.
3. Alert on `oracle_deviation_breaker` events. They are emitted *before* the
   error is returned, so a rising count means charges are being refused.
4. Treat a sudden cluster of `InsufficientBalance` failures as a possible
   `price == 1` symptom. Cross-check against `oracle_liveness` /
   `OracleLivenessEvent` and the raw oracle feed before assuming a genuine
   funding problem.
5. If a bad price is confirmed, engage the emergency stop rather than
   reconfiguring the oracle mid-flight — see
   [`emergency_stop.md`](emergency_stop.md).

## Events

For off-chain verification and indexability, the following events are emitted:

- `oracle_config_updated`: Emitted when the admin updates oracle configuration. Includes enabled status, oracle address, max acceptable age, and timestamp.
- `oracle_charge_resolved`: Emitted when a charge resolves its token target via the oracle. Includes `quote_amount`, `token_amount`, `price`, `price_timestamp` from the oracle, and resolution `timestamp`.
- `oracle_liveness`: Emitted when `emit_oracle_liveness()` is called for monitoring. Includes `last_sample_ts`, `age`, `healthy` status, and check timestamp. Allows monitoring rigs to alert before charges start failing due to stale oracle data.

## Oracle Liveness Monitoring

The contract provides a view-only `emit_oracle_liveness()` entrypoint that enables monitoring systems to verify oracle health without requiring admin privileges.

### Usage

```rust,ignore
// Check oracle health before charging
match client.emit_oracle_liveness(&env) {
    Ok(event) => {
        if event.healthy {
            // Oracle is healthy, proceed with oracle-dependent charge
            println!("Oracle healthy: age={}s, threshold={}s", event.age, event.max_age_seconds / 2);
        } else {
            // Oracle is stale or approaching staleness
            // Use fallback pricing or alert operators
            eprintln!("WARNING: Oracle stale! Age={}s exceeds healthy threshold", event.age);
        }
    }
    Err(Error::OracleNotConfigured) => {
        // Oracle not enabled, use base pricing
        println!("Oracle not configured, using base subscription amounts");
    }
    Err(e) => panic!("Unexpected error: {:?}", e),
}
```

### OracleLivenessEvent Fields

| Field            | Type   | Description                                                       |
| ---------------- | ------ | ----------------------------------------------------------------- |
| `last_sample_ts` | `u64`  | Timestamp of the latest oracle price sample                       |
| `age`            | `u64`  | Age of the sample in seconds (`current_time - last_sample_ts`)    |
| `healthy`        | `bool` | `true` if `age <= max_age_seconds / 2`, indicating healthy oracle |
| `timestamp`      | `u64`  | Ledger timestamp when this liveness check was performed           |

### Health Threshold

The `healthy` field is computed as:

```
healthy = (age <= max_age_seconds / 2)
```

This provides early warning when the oracle sample is approaching the staleness threshold. Monitoring systems can alert operators when `healthy = false`, allowing intervention before charges start failing with `OraclePriceStale` errors.

### Security Properties

- **No authentication required**: Any caller can invoke `emit_oracle_liveness()` to verify oracle health
- **View-only**: Does not modify contract state
- **Event emission**: Publishes `OracleLivenessEvent` for off-chain indexers and monitoring systems
- **Error handling**: Returns `OracleNotConfigured` if oracle is not enabled, preventing confusion

### Integration with Monitoring

Monitoring rigs can:

1. Call `emit_oracle_liveness()` on a schedule (e.g., every 60 seconds)
2. Track the `age` field to detect increasing staleness
3. Alert operators when `healthy = false` (age > max_age_seconds / 2)
4. Trigger fallback procedures before charges fail

This provides proactive oracle health monitoring, allowing operators to address issues before they impact subscription billing.

---

## OracleAdapter Architecture (Issue #477)

### Overview

Oracle pricing is now pluggable via a **strategy pattern**. The `OracleConfig` struct carries an `OracleKind` field that selects which adapter resolves the price at charge time. The public contract ABI and storage remain backwards compatible — configs without an explicit `kind` default to `Spot`.

### OracleKind

```rust
pub enum OracleKind {
    Spot,      // latest single price sample (default)
    Twap,      // median across a configurable sliding window
    FixedRate, // deterministic ratio; no oracle reads
}
```

### Configuration

The `set_oracle_config` entrypoint now accepts additional fields:

```
set_oracle_config(
    admin,
    enabled,
    oracle,          // Option<Address> — required for Spot/Twap
    max_age_seconds, // staleness threshold
    kind,            // OracleKind::Spot | Twap | FixedRate
    window_secs,     // TWAP window (ignored for Spot/FixedRate)
    fixed_numerator, // FixedRate numerator (ignored otherwise)
    fixed_denominator // FixedRate denominator, must be != 0
)
```

The `oracle_config_updated` event now includes `kind`, `window_secs`, `fixed_numerator`, and `fixed_denominator` for full auditability.

---

### SpotAdapter

Reads the latest `OraclePrice` from `oracle.latest_price()` and validates it:

- Rejects non-positive prices (`OraclePriceInvalid`).
- Rejects prices whose age exceeds `max_age_seconds` (`OraclePriceStale`).

This is the default behaviour and preserves all existing charge logic.

---

### TwapAdapter

Reads a list of `OraclePrice` observations via `oracle.get_observations(since)` for the last `window_secs` seconds.

**Median calculation** — not arithmetic mean:

```
prices = [obs.price for obs in observations if obs.age <= max_age_seconds]
sort(prices)
median = prices[len(prices) / 2]  // middle element for odd-length
```

Using the median rather than the mean means an attacker must control **more than half** of the observations inside the window to shift the output meaningfully. This resists single-block (flash-loan) price manipulation.

**Recommended configuration**

- Enforce a minimum TWAP window of at least 60 seconds in code.
- For mainnet deployments, use a default window of 5 minutes (300 seconds) or longer so that a single-block spike is diluted by several honest observations.
- Pair the TWAP with conservative slippage bounds such as 0.5%-1.0% for standard billing flows; tighter bounds may be appropriate for volatile markets or when the oracle feed itself is not well-distributed.

**Edge cases:**
| Scenario | Result |
|---|---|
| 1 observation | That price (equivalent to spot) |
| Empty window | `OraclePriceUnavailable` |
| All observations stale after filtering | `OraclePriceStale` |

---

### FixedRateAdapter

Computes a deterministic price without any oracle contract calls:

```
price = (fixed_numerator × 10^7) / fixed_denominator
```

- `fixed_denominator == 0` is rejected at configuration time with `InvalidInput`.
- Staleness and oracle address are completely ignored.
- Suitable for pegged pairs, test environments, and fee structures expressed as ratios.

**Security:** configuration changes require admin auth, so unauthorized parties cannot alter the fixed rate.

---

### Dispatch Flow

`resolve_charge_amount` now delegates to `oracle_adapter::dispatch_price`:

```
match config.kind {
    Spot      → SpotAdapter::quote()
    Twap      → TwapAdapter::quote()
    FixedRate → FixedRateAdapter::quote()
}
```

All adapters share the same `OracleAdapter` trait and return a `u128` price scaled by `10^7`. The charge math that follows is unchanged.

---

### Security Rationale

| Property                | Spot     | TWAP                  | FixedRate        |
| ----------------------- | -------- | --------------------- | ---------------- |
| Oracle reads            | Yes      | Yes                   | No               |
| Staleness enforced      | Yes      | Yes (per observation) | N/A              |
| Manipulation resistance | Low      | High (median)         | Perfect (static) |
| Oracle dependency       | Required | Required              | None             |
| Admin auth to change    | Yes      | Yes                   | Yes              |

---

## Per-merchant staleness threshold (Issue #147 / #187)

### Problem

High-frequency billing (e.g. minute/hourly intervals) needs tighter oracle
freshness than monthly plans. A single compile-time `MAX_ORACLE_AGE` cannot
express that trade-off: either short-interval merchants reject healthy quotes,
or long-interval merchants accept quotes that are too old for their risk model.

### Current behavior (already runtime-configurable)

Staleness is **not** a compile-time constant in the vault today. Admins set a
global threshold through:

```text
set_oracle_config(admin, enabled, oracle, max_age_seconds)
```

`resolve_charge_amount` rejects quotes when
`now - price.timestamp > max_age_seconds` and returns `OraclePriceStale` (`5008`)
**before** any balance mutation. The same `max_age_seconds` value is used for
oracle liveness health checks (`age <= max_age_seconds / 2`).

### Design decision for this release: keep the threshold global

We keep a **single vault-wide** `max_age_seconds` rather than adding a
per-merchant (or per-subscription) override in this change.

Rationale:

1. **One oracle feed, one freshness contract.** The vault reads a shared oracle
   adapter (`Spot` / `TWAP` / `FixedRate`). Mixing merchant-specific ages against
   the same feed makes monitoring and incident response harder: a quote can be
   "fresh" for merchant A and "stale" for merchant B in the same ledger.
2. **Circuit-breaker coherence.** Deviation checks and liveness events are keyed
   off the global config. Divergent ages would desynchronize breaker trips from
   staleness failures.
3. **Storage / migration cost.** Extending `MerchantConfig` or adding a new
   `DataKey` discriminant requires a careful storage migration and expands the
   admin surface. That is deferred until there is a clear operator demand.
4. **Security floor stays simple.** Ledger close-time skew already argues for a
   minimum safe window (≥ 60s). A global floor is easier to audit than
   per-merchant exceptions that could be set unsafely low.

### Recommended global values by billing cadence

| Cadence | Suggested `max_age_seconds` | Notes |
| --- | --- | --- |
| Sub-hourly / high-frequency | 60–120 | Stay above the 60s safety floor |
| Daily | 300–900 | Align with TWAP window when used |
| Weekly / monthly | 900–3600 | Still reject multi-hour outages |

Operators who need tighter guarantees for a subset of merchants should run a
dedicated vault instance (or oracle adapter kind) with a stricter global age,
rather than mixing thresholds in one deployment.

### Future extension (not implemented)

If per-merchant tuning becomes necessary:

1. Add optional `oracle_max_age_seconds: Option<u64>` under
   `DataKey::MerchantConfig` **or** a dedicated discriminant
   `MerchantOracleMaxAge(Address)`.
2. Define `effective_max_age(merchant) = merchant_override.unwrap_or(global)`,
   with `effective_max_age >= 60` enforced at write time.
3. Emit `oracle_config_updated` (or a merchant-scoped event) when overrides
   change; keep charge failure codes unchanged (`OraclePriceStale`).
4. Document migration / rollback: clearing the override restores global
   behavior with no evidence rewrite.

Until that ships, configure `max_age_seconds` globally to the **strictest**
merchant class on the vault.

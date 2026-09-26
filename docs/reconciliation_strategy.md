# Billing Statement Reconciliation Strategy

When billing statements are pruned (compacted), the detailed history is replaced by a `BillingStatementAggregate`. To maintain financial reporting accuracy and perform reconciliation, follow this strategy.

## 1. Aggregate Structure
The `BillingStatementAggregate` stores the summary of all pruned statements:
- `pruned_count`: Total number of rows removed.
- `total_amount`: Sum of `amount` across all removed rows.
- `totals`: Per-kind breakdown (`interval`, `usage`, `one_off`).
- `oldest_period_start` / `newest_period_end`: Time range covered by pruned data.

## 2. Reconstructing Full History
To calculate the total billed amount for a subscription since its creation:
```
Total Billed = Aggregate.total_amount + Sum(LiveStatements.amount)
```

To calculate the breakdown per charge kind:
```
Total Interval = Aggregate.totals.interval + Sum(LiveStatements where kind == Interval)
Total Usage = Aggregate.totals.usage + Sum(LiveStatements where kind == Usage)
Total One-Off = Aggregate.totals.one_off + Sum(LiveStatements where kind == OneOff)
```

## 3. Verification & Integrity
- **Sequence Integrity**: The lowest `sequence` number in the live statements should be equal to `Aggregate.pruned_count`. Any gap indicates a data integrity issue.
- **Count Consistency**: `SubscriptionVault::get_total_statements` returns the count of *live* statements. The total number of statements ever created can be inferred as `Aggregate.pruned_count + LiveCount`.
- **Amount Consistency**: The `Subscription::lifetime_charged` field should always equal the sum of all billing statements (compacted + live).
    - Note: Differences may arise if refunds were processed, which are tracked separately in `MerchantEarnings`.

## 4. Reconciliation Workflow
1. Call `get_stmt_compacted_aggregate(subscription_id)` to get the summary of pruned history.
2. Call `get_sub_statements_offset` or `get_sub_statements_cursor` to fetch live detailed rows.
3. Sum the values as described above.
4. Compare against `get_subscription(subscription_id).lifetime_charged` for high-level validation.

---

# Post-Emergency-Stop Reconciliation Runbook

> **Entry point for this section:** [`emergency_stop.md`](emergency_stop.md) →
> *Deactivation Procedure* → *Step 3* and *Post-Incident*. Lifting the
> emergency stop is the moment contract state and off-chain expectations are
> guaranteed to disagree. This runbook is the procedure for closing that gap.
>
> It is deliberately separate from the normal charge-failure and network-issue
> workflows above: those assume a running billing loop, and during an
> emergency stop there is no billing loop at all.

## 5. Why a stop changes the reconciliation problem

`enable_emergency_stop` blocks `charge_subscription`, `batch_charge`,
`charge_usage`, `create_subscription` and `deposit_funds`. It does **not**
roll anything back, and it does **not** advance any clock. For the duration of
the stop:

- **Charges were skipped, not deferred.** No charge was queued. When the stop
  lifts, the contract does not "catch up" — the next charge simply becomes due
  when `now >= last_payment_timestamp + interval_seconds` first becomes true.
- **`last_payment_timestamp` is frozen.** Every skipped interval is a longer
  gap between two real charges.
- **`period_index` jumps.** The next charge lands on a much later period index
  than the one in progress when the stop engaged. Anything that assumed
  contiguous periods (statements, aggregates, an off-chain ledger) will show a
  discontinuity here.
- **Wall-clock ran; the ledger did not bill.** Subscribers were billed nothing
  for the stop duration, so any off-chain system that accrued an expectation
  per unit time is now overstated.
- **The emergency-stop gate is checked before the nonce**, so *no* nonce was
  consumed by blocked calls. A nonce held across the stop is still valid — see
  [§8.3](#83-nonce-handling-across-the-stop).

The safe recovery behaviour in `emergency_stop.md` (withdrawals and pauses
still available) means **funds did not move during the stop** unless a merchant
or subscriber used those entrypoints. Reconcile for that possibility explicitly
— it is the one class of state change a stop does permit.

## 6. Pre-lift gate: do not disable the stop until these pass

Run all of these **while the stop is still engaged**. Every one of them is a
read-only query, so they are all available under the stop (see *Safe Recovery
Behavior* in `emergency_stop.md`). Deactivating first and reconciling after is
strictly worse: once charging resumes, any imbalance is immediately compounded.

- [ ] `get_token_reconciliation(token).is_balanced` is `true` for every accepted token
- [ ] `contract_balance == total_prepaid + total_merchant_liabilities + recoverable_amount`
- [ ] `get_contract_reconciliation_summary` reports no `is_balanced == false` across the full token set
- [ ] A reconciliation proof has been captured for each token: `generate_reconciliation_proof(token)`, stored off-chain with its `ledger_sequence`
- [ ] The root cause is identified and patched (deactivating without a fix re-arms the bug on the first billing run)
- [ ] If an oracle or external feed was implicated, the deviation breaker is armed — see [`oracle_pricing.md`](oracle_pricing.md) → *Protection: arm the deviation circuit breaker*

That proof snapshot is your **baseline**. Every number in §8 is a delta against
it, so capture it now.

## 7. Freeze the billing engine before lifting the stop

`disable_emergency_stop` restores `batch_charge` immediately. If your billing
engine is polling, it will fire a catch-up batch within one poll interval —
before you have classified anything, and with whatever nonce it last read.

Order matters:

1. **Stop the scheduler first.** Take the billing job out of its loop. Do not
   rely on the emergency stop as the gate; it is about to be removed.
2. Export the pending-charge queue (ids, intervals, `last_payment_timestamp`).
3. Confirm `get_emergency_stop_status()` is still `true`.
4. Only then `disable_emergency_stop`.

Leave the scheduler down until §9 is complete. A half-reconciled run is harder
to unwind than a late one.

## 8. Post-lift reconciliation

### 8.1 Snapshot the delta

For every accepted token, compare against the §6 baseline:

| Field | Question |
|-------|----------|
| `contract_balance` | Did anything move? An increase during the stop means direct transfers or a successful withdrawal path. |
| `total_prepaid` | Did subscribers top up? (`deposit_funds` was blocked, so any increase is post-lift or a direct transfer.) |
| `total_merchant_liabilities` | Did a merchant withdraw earnings earned before the stop? Possible — `withdraw_merchant_funds` stayed available. |
| `recoverable_amount` | Any non-zero value is an unaccounted surplus. It is **not** free money; see [`recovery.md`](recovery.md) before touching it. |

Re-verify `is_balanced` for every token immediately after lifting. A balance
that was correct under the stop and is incorrect after it points at something
that moved between the two snapshots — a merchant withdrawal, a direct token
transfer, or a missed export.

### 8.2 Account for every missed interval

The core question: **what should have been charged, and what was actually
charged?**

```
missed_intervals(sub) = floor((lift_timestamp - last_payment_before_stop) / interval)
catch_up_charge(sub)  = charge_now - last_charge_before_stop   // ONE charge
```

Note the asymmetry: a missed interval does **not** produce N charges. The
contract charges `subscription.amount` **once**, on the next successful
interval charge, regardless of how many intervals elapsed. Catch-up is
`one interval's worth of billing`, not `N ×`.

Classify each subscription in the queue:

| Category | Definition | Action |
|----------|------------|--------|
| **Caught up** | `last_payment_timestamp + interval <= lift_timestamp` | Charge normally. It is simply due. |
| **Not yet due** | `last_payment_timestamp + interval > lift_timestamp` | **Do not charge.** Expect `IntervalNotElapsed` (4004). Remove from the queue. |
| **Underfunded** | `prepaid_balance < amount` | Charge normally; expect `InsufficientBalance` (5001) and a `GracePeriod` / `InsufficientBalance` transition. Re-queue on `RecoveryReadyEvent` / `SubscriptionResumedEvent`. |
| **No longer billable** | `Paused` / `Cancelled` / `Expired` | Expect `NotActive` (4002) or `NotFound` (2001). Remove from the queue. |
| **Suspect** | Any id whose off-chain expectation disagrees with its on-chain `lifetime_charged` | Escalate before charging. Do not auto-retry. |

Build this classification from `get_subscription(id)` and
`get_next_charge_info(id)` for each queued id. `get_next_charge_info` returns
the contract's own `next_charge_timestamp` — use it rather than recomputing
`last_payment + interval` off-chain, so you cannot disagree with the contract
about the boundary.

### 8.3 Nonce handling across the stop

The emergency-stop gate runs **before** the nonce check, so blocked calls never
consumed a nonce.

- A nonce read before the stop and still unused is **still valid** after the
  lift. It is safe to reuse for a single retry.
- Do not assume this. A nonce can also have been consumed by an unrelated
  admin operation in domain 1 during the stop window.
- Re-read with `get_admin_nonce(admin, 1)` immediately before each `batch_charge`
  and use the value you just read. This costs one read and removes an entire
  class of `NonceAlreadyUsed` (1005) failures.
- Every batch that *runs* consumes its nonce, whether or not any item
  succeeded. See
  [`integration_guide.md`](integration_guide.md) → *Retry guidance for partial
  failures*.

### 8.4 Statement and aggregate continuity

Re-run the §1–§3 checks specifically for the stop window:

- `Aggregate.pruned_count` versus the lowest live `sequence`. A gap here means
  compaction ran across the stop boundary and a period was not accounted.
- `Subscription.lifetime_charged` versus the sum of compacted plus live
  statements. Because catch-up is a single charge, the expected statement count
  for the window is **1 per subscription that was actually due** — not one per
  elapsed interval. If your off-chain model expected N, that is the bug, not
  the contract.
- Any off-chain accrual that ran on a wall-clock timer must be reconciled
  against `lifetime_charged`, which is the authority. Rebase or void the
  off-chain figure; do not "catch up" the subscriber on-chain for time that
  passed while the stop was engaged.

## 9. Resumption checklist

Only after §8 is complete for every token in scope:

- [ ] `is_balanced == true` for every accepted token
- [ ] Every queued subscription is classified into exactly one §8.2 category
- [ ] No subscription in "Suspect"
- [ ] Off-chain accruals rebased or voided for the stop window
- [ ] Statement/aggregate continuity verified (§8.4)
- [ ] Reconciliation proofs regenerated and stored with the new `ledger_sequence`
- [ ] Billing engine restarted **after** the rebase, not before
- [ ] First post-lift batch manually reviewed — do not let it run unattended
- [ ] Monitoring re-armed: `oracle_liveness`, `EmergencyStop*` events, and any
      alerting that was disabled during the incident

## 10. Post-incident record

- Link the §6 baseline proof and the §9 final proof.
- Record the lift `ledger_sequence` and timestamp.
- Record the batch nonces used for the catch-up run, so a future reconciliation
  can distinguish the catch-up from ordinary billing.
- Note any subscription left in `Suspect` and its resolution — those are the
  entries most likely to resurface in an audit.
- Feed the observed behaviour back into this runbook.

## References

- Emergency stop semantics and the pre-lift read surface:
  [`emergency_stop.md`](emergency_stop.md)
- Per-charge semantics, skip conditions, and batch limits:
  [`batch_charge.md`](batch_charge.md)
- Retry rules for the catch-up run:
  [`integration_guide.md`](integration_guide.md)
- Unaccounted surplus and stranded funds:
  [`recovery.md`](recovery.md)
- Oracle staleness / deviation after an external-feed incident:
  [`oracle_pricing.md`](oracle_pricing.md)
- Interval enforcement and the 60 s floor:
  [`billing_intervals.md`](billing_intervals.md)

---

# Contract-Level Reconciliation Queries

The contract provides read-only endpoints for off-chain auditors to validate the accounting equation:

```
contract_token_balance = total_prepaid + total_merchant_liabilities + recoverable
```

## API Overview

### 1. Token-Level Reconciliation: `get_token_reconciliation(token)`

Returns complete reconciliation data for a single settlement token.

**Response: `TokenLiabilities`**
- `token`: Token contract address
- `total_prepaid`: Sum of all subscriber prepaid balances
- `total_merchant_liabilities`: Sum of all merchant earnings (accruals - withdrawals - refunds)
- `recoverable_amount`: Stranded funds that can be recovered by admin
- `contract_balance`: Actual token balance held by the contract
- `computed_total`: Prepaid + merchant liabilities + recoverable
- `is_balanced`: Whether the accounting equation validates

**Usage:**
```rust
let reconciliation = client.get_token_reconciliation(&usdc_token);
assert!(reconciliation.is_balanced);
assert_eq!(
    reconciliation.contract_balance,
    reconciliation.total_prepaid
        + reconciliation.total_merchant_liabilities
        + reconciliation.recoverable_amount
);
```

### 2. Multi-Token Summary: `get_contract_reconciliation_summary(start_token_index, limit)`

Returns paginated reconciliation data for all accepted tokens.

**Parameters:**
- `start_token_index`: Index into accepted tokens list (0 for first page)
- `limit`: Maximum summaries to return (capped at 50)

**Response: `ReconciliationSummaryPage`**
- `token_summaries`: Vector of `TokenLiabilities`
- `next_token_index`: Cursor for next page, `None` when complete

**Usage:**
```rust
// Get all token reconciliations
let mut index = 0u32;
loop {
    let page = client.get_contract_reconciliation_summary(&index, &50);
    for summary in &page.token_summaries {
        println!("Token: {:?}, Balanced: {}", summary.token, summary.is_balanced);
    }
    match page.next_token_index {
        Some(next) => index = next,
        None => break,
    }
}
```

### 3. Auditable Proof Generation: `generate_reconciliation_proof(token)`

Creates an auditable snapshot with all data needed to independently validate the accounting equation.

**Response: `ReconciliationProof`**
- `timestamp`: Ledger timestamp when proof was generated
- `ledger_sequence`: Ledger sequence for temporal anchoring
- `token`: Token being audited
- `contract_balance`: Contract's token balance
- `total_prepaid`: Sum of all subscriber prepaid balances
- `total_merchant_liabilities`: Total merchant earnings liabilities
- `computed_recoverable`: Calculated recoverable amount
- `subscription_count`: Number of subscriptions scanned
- `merchant_count`: Number of merchants with earnings
- `is_valid`: Whether the accounting equation validates

**Security Properties:**
- Read-only: Cannot modify contract state
- Temporally anchored: Includes ledger sequence
- Self-contained: All validation data in one struct

### 4. Paginated Prepaid Query: `query_prepaid_balances_paginated(request)`

Bounded-compute query for aggregating prepaid balances across subscriptions.

**Request: `PrepaidQueryRequest`**
- `token`: Token to filter by (required)
- `start_subscription_id`: Starting subscription ID (inclusive)
- `scan_limit`: Maximum subscriptions to scan (capped at 500)

**Response: `PrepaidQueryResult`**
- `token`: Token queried
- `partial_total`: Sum of prepaid balances in scan window
- `subscriptions_count`: Number of subscriptions with non-zero prepaid
- `next_start_id`: Next ID to scan, `None` if complete
- `has_more`: Whether more subscriptions exist beyond window

**Off-Chain Aggregation Example:**
```rust
let mut total_prepaid = 0i128;
let mut start_id = 0u32;

loop {
    let result = client.query_prepaid_balances_paginated(&PrepaidQueryRequest {
        token: usdc_token.clone(),
        start_subscription_id: start_id,
        scan_limit: 500,
    });

    total_prepaid += result.partial_total;

    if !result.has_more {
        break;
    }
    start_id = result.next_start_id.unwrap();
}
```

## Reconciliation Workflow for Auditors

### Quick Validation (Single Token)
```rust
// 1. Get reconciliation data
let recon = client.get_token_reconciliation(&token);

// 2. Verify accounting equation
assert!(recon.is_balanced, "Accounting equation does not balance!");

// 3. Verify specific amounts
assert_eq!(
    recon.contract_balance,
    recon.total_prepaid + recon.total_merchant_liabilities + recon.recoverable_amount
);
```

### Full Audit with Proof Generation
```rust
// Generate proof for record keeping
let proof = client.generate_reconciliation_proof(&token);

// Store proof off-chain with ledger sequence for temporal reference
store_audit_record(proof.ledger_sequence, proof);

// Verify at a later date
let current = client.get_token_reconciliation(&token);
assert_eq!(current.contract_balance, proof.contract_balance); // Or investigate changes
```

### Multi-Token Portfolio Reconciliation
```rust
let mut all_balanced = true;
let mut start_index = 0u32;

loop {
    let page = client.get_contract_reconciliation_summary(&start_index, &50);

    for summary in &page.token_summaries {
        if !summary.is_balanced {
            all_balanced = false;
            log_imbalance(&summary.token, summary);
        }
    }

    match page.next_token_index {
        Some(next) => start_index = next,
        None => break,
    }
}

assert!(all_balanced, "Some tokens have accounting imbalances!");
```

## Performance & Security Considerations

### Bounded Compute
- `MAX_PREPAID_SCAN_DEPTH = 500`: Limits subscription scans per call
- `MAX_TOKEN_SUMMARIES_PER_PAGE = 50`: Limits token summaries per call
- Indexers should chain paginated calls to build complete totals

### Gas Efficiency
- `get_token_reconciliation`: O(subscriptions + merchants) — use for spot checks
- `generate_reconciliation_proof`: Same complexity but returns compact proof
- `query_prepaid_balances_paginated`: O(scan_limit) — bounded and predictable

### Read-Only Safety
All reconciliation endpoints are read-only and cannot modify contract state. They:
- Require no authentication
- Emit no events
- Have no side effects
- Are safe to call at any time

## Indexer Integration

Indexers computing off-chain proofs should:

1. **Use paginated queries** for large datasets
2. **Validate proofs** against on-chain data periodically
3. **Store ledger sequences** with proof records for temporal validation
4. **Monitor `is_balanced`** for anomaly detection
5. **Aggregate across pages** to verify total contract liabilities

Example indexer proof computation:
```rust
// 1. Collect paginated prepaid data
let prepaid_total = aggregate_paginated_prepaid(&client, &token);

// 2. Get merchant liabilities (from indexed data or contract)
let merchant_total = get_indexed_merchant_liabilities(&token);

// 3. Get contract balance from token contract
let contract_balance = token_client.balance(&vault_address);

// 4. Compute and verify
let recoverable = contract_balance - prepaid_total - merchant_total;
assert!(recoverable >= 0, "Negative recoverable indicates data inconsistency");
```

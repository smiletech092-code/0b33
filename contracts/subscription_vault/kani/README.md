# Kani Formal Verification Harnesses

This directory contains [Kani](https://model-checking.github.io/kani/) proof
harnesses for the `subscription_vault` contract. Kani is a model-checking tool
that exhaustively verifies Rust programs over all reachable inputs, going beyond
what property-based fuzz tests can cover.

## Running the proofs

```bash
cargo kani --manifest-path contracts/subscription_vault/Cargo.toml
```

Individual harness:

```bash
cargo kani --harness cancel_refund_bounded \
  --manifest-path contracts/subscription_vault/Cargo.toml
```

---

## Harnesses

### `cancel_refund.rs`

Verifies the `compute_cancel_refund` pure helper in `src/subscription.rs`,
which calculates the amount returned to a subscriber when they cancel a
subscription.

#### Properties proved

| Harness | Property |
|---|---|
| `cancel_refund_bounded` | For every `i128` balance the refund is `≤ balance` (no over-refund) and `== balance` (no hidden deduction). |
| `cancel_refund_zero_balance` | A zero balance produces a zero refund. |
| `cancel_refund_max_balance` | `i128::MAX` balance does not overflow and returns `i128::MAX`. |

`cancel_refund_bounded` uses `kani::any()` to range over the full `i128`
domain, giving exhaustive coverage rather than sampled coverage.

#### Security relevance

The cancellation path moves `prepaid_balance` out of escrow and back to the
subscriber. An arithmetic bug here could allow a drain (refund > escrowed
amount) or silently shortchange the subscriber. The proofs rule out both
outcomes for all possible balance values.

#### Assumptions

- `compute_cancel_refund` is a pure function with no Soroban host calls. The
  harness therefore runs without a simulated host environment. Any future
  change that introduces host calls (storage reads, token transfers) into this
  function would require the harness to be updated or split.
- No assumption is placed on the input value — `kani::any::<i128>()` covers
  negative, zero, positive, and boundary values.

#### Known limitations / out of scope

- The harnesses do not verify the *caller* `apply_cancellation` in
  `src/subscription.rs`, which performs the full CEI sequence (state write →
  escrow creation → token transfer). That control flow is not amenable to Kani
  without a Soroban host stub.
- The dispute lifecycle, charge arithmetic, and withdrawal paths are not yet
  covered by Kani proofs. Relevant candidates for future harnesses are noted
  below.

---

## Coverage gaps and candidates for future harnesses

| Area | File | Candidate property |
|---|---|---|
| Charge arithmetic | `src/charge_core.rs` | Charged amount never exceeds `prepaid_balance`; no overflow in interval math. |
| Safe math helpers | `src/safe_math.rs` | `safe_add` / `safe_sub` never silently wrap. |
| Dispute escrow | `src/dispute.rs` | `total_disbursed` never exceeds `original_amount`. |
| Next-charge time | `src/subscription.rs` | `next_charge_time` never overflows for validated intervals. |

Contributions adding new harnesses should follow the pattern in
`cancel_refund.rs`: extract the arithmetic into a pure helper, re-export it
from `lib.rs` under `#[cfg(kani)]` or `pub`, and write a focused harness in
this directory.

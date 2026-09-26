---
title: "docs/recovery.md: document the get_token_reconciliation query API"
labels: [docs, recovery, api]
area: docs/recovery.md
---

## Summary

`docs/recovery.md` references monitoring and auditing but only provides pseudocode. The
contract exposes a public, unauthenticated `get_token_reconciliation` function that returns a
full `TokenLiabilities` breakdown. This API is undocumented, so operators have no guidance on
how to programmatically detect stranded funds before initiating recovery.

## What is missing

- The `get_token_reconciliation` function signature and its location in the public ABI.
- Full field-by-field documentation of `TokenLiabilities`.
- A validation workflow (check `is_balanced` before acting on `recoverable_amount`).
- A concrete CLI invocation example.

## Suggested addition

Replace the pseudocode block in the "Monitoring and Auditing" section with a **Identifying
Stranded Funds: Query API** section:

```markdown
## Identifying Stranded Funds: Query API

```rust
pub fn get_token_reconciliation(env: &Env, token: Address) -> TokenLiabilities
```

No authentication required. Returns the current reconciliation state for one token.

### TokenLiabilities fields

| Field | Type | Meaning |
|---|---|---|
| `total_prepaid` | i128 | Sum of all subscription prepaid balances |
| `total_merchant_liabilities` | i128 | Sum of all merchant balances |
| `recoverable_amount` | i128 | Stranded funds (contract_balance − liabilities) |
| `contract_balance` | i128 | Actual token balance held by contract |
| `computed_total` | i128 | prepaid + merchant + recoverable |
| `is_balanced` | bool | True if contract_balance == computed_total |
| `normalized_*` | u64 | Human-readable versions divided by token decimals |

### Validation workflow

```rust
let liabilities = client.get_token_reconciliation(&token);

// 1. Abort if contract is out of balance
if !liabilities.is_balanced {
    panic!("Contract imbalance — investigate before recovery");
}

// 2. Check for stranded funds
if liabilities.recoverable_amount > 0 {
    println!("Stranded: {} (normalized: {})",
        liabilities.recoverable_amount,
        liabilities.normalized_recoverable);
}
```

### CLI example

```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  --rpc-url https://soroban-testnet.stellar.org \
  -- \
  get_token_reconciliation \
  --token <TOKEN_ADDRESS>
```
```

## Acceptance criteria

- [ ] `docs/recovery.md` documents `get_token_reconciliation` with full field descriptions.
- [ ] The `is_balanced` check is shown as a prerequisite before using `recoverable_amount`.
- [ ] A CLI invocation example is included.

## References

- Source: `docs/recovery_doc_gaps.md` Gap 3 (issue #240)
- Implementation: `contracts/subscription_vault/src/lib.rs` line 661,
  `contracts/subscription_vault/src/queries.rs` lines 532–570

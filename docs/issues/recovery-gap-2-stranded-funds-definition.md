---
title: "docs/recovery.md: add precise definition of stranded funds and the accounting model"
labels: [docs, recovery, accounting]
area: docs/recovery.md
---

## Summary

`docs/recovery.md` uses the term "stranded funds" without a rigorous definition. Without a
clear formula, operators cannot distinguish stranded tokens from legitimately accounted ones,
making it impossible to audit or reason about recovery safety.

## What is missing

- The formula `stranded(token) = contract_balance(token) − accounted(token)`.
- Definition of "accounted" (subscription prepaid balances + merchant accumulated balances).
- The solvency invariant that must hold before recovery can proceed.
- Concrete examples of how funds become stranded.

## Suggested addition

Add an **Accounting Model: Defined vs. Stranded Funds** section to `docs/recovery.md`:

```markdown
## Accounting Model: Defined vs. Stranded Funds

### Accounted funds

Every token held by the contract must be linked to one of:

1. **Subscription prepaid balances** — amount deposited by the subscriber, decremented by
   charges.
2. **Merchant accumulated balances** — amount credited through charges, withdrawable by the
   merchant.

```
accounted(token) = Σ subscription.prepaid_balance + Σ merchant_balance
```

### Stranded funds

Funds physically in the contract but not linked to any subscription or merchant:

```
stranded(token) = contract_balance(token) − accounted(token)
```

Stranded funds must be ≥ 0. A negative value indicates insolvency — a critical invariant
violation that must be investigated before any recovery attempt.

### Examples of stranded scenarios

| Scenario | Reason |
|---|---|
| User sends tokens directly to contract address | Not attached to any subscription |
| Upgrade bug leaves tokens in deprecated storage key | Key no longer indexed |
| Cancelled subscription with lost subscriber keys | Prepaid balance unreachable |
| Transfer fails mid-flight | Tokens moved but state update rolled back |

### Solvency invariant

For every supported token:

```
contract_balance(token) >= accounted(token) >= 0
```

If this invariant is violated, do not proceed with recovery.
```

## Acceptance criteria

- [ ] `docs/recovery.md` includes the stranded formula with variable definitions.
- [ ] The solvency invariant is stated explicitly.
- [ ] At least two concrete stranded-fund scenarios are listed.

## References

- Source: `docs/recovery_doc_gaps.md` Gap 2 (issue #240)
- Implementation: `contracts/subscription_vault/src/admin.rs` lines 528–562,
  `contracts/subscription_vault/src/queries.rs` lines 530–570

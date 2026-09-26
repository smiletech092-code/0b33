---
title: "docs/recovery.md: document the admin-only authorization model"
labels: [docs, recovery, auth]
area: docs/recovery.md
---

## Summary

`docs/recovery.md` mentions admin authorization in passing but does not explain the
authentication mechanism, signature requirements, or multi-signature support. Operators and
auditors cannot verify the security model from the documentation alone.

## What is missing

- What `require_admin_auth` actually does (cryptographic signature check + stored-address match).
- Whether multi-sig accounts are supported and how threshold signing is handled by Soroban's auth
  layer.
- Code-level reference so readers can cross-check the implementation.

## Suggested addition

Add an **Authorization Model** subsection to the "Technical Implementation" section of
`docs/recovery.md`:

```markdown
## Authorization Model

`recover_stranded_funds` is admin-only and requires both:

1. **Cryptographic signature verification** — The caller must sign with the admin address.
   Soroban's `Address::require_auth()` validates this on-chain.
2. **Admin address validation** — The signing address must match the admin address stored in
   contract state (set at initialization or updated via admin rotation).

### Multi-signature support

Stellar accounts support multiple signers with a threshold. If the admin is configured as a
multi-sig account, recovery automatically requires the threshold number of signatures.
Soroban's auth layer handles threshold validation transparently.

### Implementation reference

`contracts/subscription_vault/src/admin.rs` — `do_recover_stranded_funds`:

    require_admin_auth(env, &admin)?;
    //  1. calls admin.require_auth()  → Soroban verifies signature
    //  2. checks stored_admin == admin → returns Error::Unauthorized if mismatch
```

## Acceptance criteria

- [ ] `docs/recovery.md` contains an Authorization Model section explaining both checks.
- [ ] Multi-sig behaviour is documented.
- [ ] Implementation reference (file + function) is included.

## References

- Source: `docs/recovery_doc_gaps.md` Gap 1 (issue #240)
- Implementation: `contracts/subscription_vault/src/admin.rs` lines 528–540

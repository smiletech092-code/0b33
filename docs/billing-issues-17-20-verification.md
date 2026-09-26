# Verification: issues #17–#20 already fixed

These four billing validation issues were investigated and found to already be
resolved by existing code on `main`. No functional changes were required.

## #19 — `create_subscription` does not reject `amount = 0`
Already enforced in `do_create_subscription_with_token`:
`contracts/subscription_vault/src/subscription.rs:599-601` returns
`Error::InvalidAmount` (3001) when `amount == 0`.

## #20 — `interval_seconds = 0` allows unlimited same-block charges
Already enforced via `validate_interval`
(`contracts/subscription_vault/src/subscription.rs:101-108`), which rejects any
`interval_seconds` below `MIN_SUBSCRIPTION_INTERVAL_SECONDS` (60). This is
called from `do_create_subscription_with_token` before a subscription is
created, returning `Error::InvalidInput` (3002).

## #18 — `last_payment_timestamp` updated by usage charges
Not occurring: the only write to `last_payment_timestamp` in
`contracts/subscription_vault/src/charge_core.rs` is inside `charge_one`
(interval charges), at line 582. `charge_usage_one` never assigns it.

## #17 — `GracePeriod` status not persisted to storage
Already persisted: `charge_core.rs` calls `write_subscription(env,
subscription_id, &sub)` immediately after transitioning `sub.status` to
`GracePeriod` (around line 745), before any event is published.

Closes #17, #18, #19, #20.

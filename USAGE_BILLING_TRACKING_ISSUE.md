# Tracking issue: usage billing complete

## Summary

`charge_usage_one` exists in `contracts/subscription_vault/src/charge_core.rs`, but the full usage-billing workflow is not yet complete end-to-end. The contract currently includes the core charge logic, but the surrounding flow for usage reporting, cap enforcement, rate limiting, and operational completion is still incomplete.

This issue tracks the remaining work needed to declare the feature "usage billing complete."

## What is done

The following behaviors are already implemented in the contract and covered by tests:

- `charge_usage_one` validates the subscription and rejects invalid usage attempts.
  - subscription exists and is active
  - `usage_enabled` is true
  - `usage_amount > 0`
  - merchant/subscriber/blocklisted checks
  - merchant pause / vacation guards
  - expiration and lifetime-cap guards
  - insufficient prepaid balance checks
- Replay protection is implemented using a reference-based idempotency key.
  - duplicate usage references for the same subscription are rejected with `Replay`.
- Usage limits are tracked via `UsageLimits` and `UsageState`.
  - burst minimum interval enforcement
  - rate-limit window enforcement
  - period-based usage-cap enforcement
  - state reset on window/period rollover
- Lifetime cap enforcement is incorporated in the usage path.
  - crossing the cap is treated as a cancel/terminal condition without debiting funds on overrun
  - exact boundary behavior is covered in tests
- Fee routing and balance accounting are wired into usage charges.
  - prepaid balance is debited
  - merchant/sub-account credits are applied
  - protocol fees are routed as expected
  - relevant events are emitted
- Core test coverage exists for basic usage charging and guard conditions.
  - successful usage charge with reference
  - replay rejection
  - burst limit exceed
  - rate limit exceed
  - usage cap exceed
  - exact boundary allowance and rollover semantics

## What is missing / incomplete

The end-to-end flow is still incomplete in several places:

1. Usage reporting pipeline is not fully defined or completed.
   - There is no fully specified, production-grade off-chain reporting path from metering agents to the contract call.
   - The contract supports a single charge call, but the operational process for batched/streaming usage submissions is still under-specified.
   - There is no complete reconciliation/reporting layer that ties reported usage volumes to billing results and settlement.

2. Full cap semantics are not fully closed out.
   - The code enforces lifetime-cap and periodic usage-cap checks, but the cross-product guarantees are not fully documented and validated in a single, end-to-end workflow.
   - Edge cases around exact-hit boundaries, cancellation timing, and status transitions still need a final audit across all billing modes.

3. Rate limiting is implemented but not clearly complete as a product contract.
   - Burst and window checks exist, but the operational semantics for multiple billing sources, time skew, and retry behavior need formal definition.
   - The contract uses approximate sliding-window logic; this may be enough for MVP but is not yet a complete production billing policy definition.

4. Usage-billing documentation and acceptance criteria are not yet fully unified.
   - The docs describe the basics of usage billing, but they do not yet fully describe the complete operational lifecycle: metering, reporting, retry, replay, dedupe, cap enforcement, and reconciliation.
   - There is not yet a single authoritative checklist that states when usage billing is considered complete.

5. End-to-end validation is still missing for the real workflow.
   - The contract has isolated tests for usage charge logic, but not a complete end-to-end scenario covering:
     - usage ingestion from an external meter
     - charge submission with a unique reference
     - rejection on replay/burst/rate/cap conditions
     - refund / accounting reconciliation
     - final status transitions and merchant reporting

## Proposed acceptance criteria for "usage billing complete"

The feature should only be considered complete when all of the following are true:

- [ ] A complete usage reporting flow exists from external metering/reporting components to contract invocation.
- [ ] Every usage charge call is idempotent by reference, and duplicate submissions are rejected deterministically.
- [ ] Burst limits, rate limits, and usage-cap limits are enforced with clear semantics for boundary values.
- [ ] Lifetime cap logic is enforced consistently for both exact-hit and overrun cases without producing financial side effects on rejected over-cap attempts.
- [ ] Subscription status transitions are correct after usage charge success, insufficient balance, cancellation, and expiry.
- [ ] Usage charges debit prepaid balance, credit merchants/sub-accounts correctly, and route fees per the configured policy.
- [ ] All expected events are emitted and are sufficient for off-chain monitoring and reconciliation.
- [ ] A final end-to-end test covers a realistic usage billing scenario from meter report to final balance and status.
- [ ] Documentation clearly defines the supported usage billing lifecycle, operational constraints, and failure modes.
- [ ] No known usage-billing gap remains for the supported product surface.

## Definition of done

Usage billing is complete when the core contract logic, off-chain reporting flow, operational invariants, and documentation all agree on the same semantics, and the end-to-end workflow has been validated with targeted tests and final reconciliation checks.

## Notes

This issue should be used as the checklist for closing the remaining work. The current status is: the charge primitive is largely implemented and tested, but the full operational flow is not yet accepted as complete.

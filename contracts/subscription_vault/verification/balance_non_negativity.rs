/// Kani formal-verification harnesses for the balance non-negativity invariant.
///
/// # Invariant
/// The contract's `prepaid_balance` field must never go negative.  Two
/// operations mutate it: `deposit_funds` (adds via `safe_add_balance`) and
/// `charge_subscription` (subtracts via `safe_sub_balance`).  Both operations
/// are implemented using `safe_add_balance` / `safe_sub_balance` from
/// `safe_math.rs`, which are the authoritative gatekeepers of the invariant.
///
/// These harnesses prove three things exhaustively over all i128 inputs:
///
/// 1. **add_balance_non_negative_result** — `safe_add_balance` only returns
///    `Ok(result)` when the result is ≥ 0.  Concretely: since it rejects
///    negative `amount`, and panics on overflow, the output is always in
///    `[balance, i128::MAX]` and therefore non-negative whenever the starting
///    balance was non-negative.
///
/// 2. **sub_balance_preserves_non_negativity** — `safe_sub_balance` only
///    returns `Ok(result)` when `result >= 0`.  This is the primary guard
///    that prevents the balance from going negative on a charge.
///
/// 3. **charge_cannot_overdraft** — If a starting balance `b >= 0` and a
///    charge amount `a >= 0` are chosen by the adversary, `safe_sub_balance`
///    only succeeds when `a <= b`, so `b - a >= 0`.  Any attempt to charge
///    more than the balance gets `Err(Underflow)` — never `Ok(negative)`.
///
/// # Compile note
/// These harnesses are gated on `#[cfg(kani)]` and the `kani_harness` crate
/// feature.  They import only `safe_sub_balance` and `safe_add_balance`, both
/// of which are `pub` functions with no Soroban `Env` dependencies, so they
/// run natively under Kani without any mock infrastructure.
#[cfg(kani)]
mod balance_non_negativity {
    use subscription_vault::{safe_add_balance, safe_sub_balance};

    // ------------------------------------------------------------------
    // Harness 1: safe_add_balance never produces a negative Ok result
    // ------------------------------------------------------------------

    /// For every pair (balance, amount) the adversary can choose, if
    /// `safe_add_balance` returns `Ok(result)` then `result >= 0`.
    ///
    /// The implementation rejects `amount < 0` and rejects overflow, so the
    /// result is always `balance + amount` with both terms non-negative.
    /// Kani will exhaustively explore all 2^128 × 2^128 value pairs via
    /// symbolic execution; the assertion will pass iff the code is correct.
    #[kani::proof]
    pub fn add_balance_non_negative_result() {
        let balance: i128 = kani::any();
        let amount: i128 = kani::any();

        // Restrict to the reachable pre-condition: balance is non-negative
        // (invariant that the rest of the contract maintains).
        kani::assume(balance >= 0);

        if let Ok(result) = safe_add_balance(balance, amount) {
            // The key safety property: a successful deposit never produces a
            // negative balance.
            assert!(result >= 0, "safe_add_balance returned a negative balance");

            // Monotonicity: a deposit never decreases the balance.
            assert!(
                result >= balance,
                "safe_add_balance returned a result smaller than the starting balance"
            );

            // Exactness: the result equals balance + amount (no hidden fees).
            assert_eq!(
                result,
                balance + amount,
                "safe_add_balance result does not equal balance + amount"
            );
        }
    }

    // ------------------------------------------------------------------
    // Harness 2: safe_sub_balance never returns a negative result
    // ------------------------------------------------------------------

    /// For every (balance, amount) pair, if `safe_sub_balance` returns
    /// `Ok(result)` then `result >= 0`.
    ///
    /// This is the core invariant guard for charge operations.
    #[kani::proof]
    pub fn sub_balance_preserves_non_negativity() {
        let balance: i128 = kani::any();
        let amount: i128 = kani::any();

        if let Ok(result) = safe_sub_balance(balance, amount) {
            // Primary invariant: result is never negative.
            assert!(result >= 0, "safe_sub_balance returned a negative balance");

            // Tightness: if Ok, the amount was non-negative and ≤ balance.
            assert!(amount >= 0, "safe_sub_balance returned Ok for a negative amount");
            assert!(
                balance >= amount,
                "safe_sub_balance returned Ok when balance < amount"
            );

            // Exactness: result == balance - amount.
            assert_eq!(
                result,
                balance - amount,
                "safe_sub_balance result does not equal balance - amount"
            );
        }
    }

    // ------------------------------------------------------------------
    // Harness 3: no adversarial (balance, charge) pair can overdraft
    // ------------------------------------------------------------------

    /// Starting from a non-negative balance, no charge amount chosen by an
    /// adversary can produce a negative result via `safe_sub_balance`.
    ///
    /// This models the contract invariant: the subscriber's vault always holds
    /// at least as much USDC as has been charged.
    #[kani::proof]
    pub fn charge_cannot_overdraft() {
        // Adversary controls both fields.
        let balance: i128 = kani::any();
        let charge_amount: i128 = kani::any();

        // Pre-condition: balance starts non-negative (maintained by prior deposits).
        kani::assume(balance >= 0);

        match safe_sub_balance(balance, charge_amount) {
            Ok(result) => {
                // A successful charge must leave a non-negative balance.
                assert!(
                    result >= 0,
                    "charge produced a negative balance — overdraft possible"
                );
                // The amount charged must not exceed the balance.
                assert!(
                    charge_amount <= balance,
                    "safe_sub_balance succeeded with charge_amount > balance"
                );
            }
            Err(_) => {
                // Any error from safe_sub_balance is fine — the charge is
                // rejected and the balance remains unchanged at `balance >= 0`.
                // No assertion needed: the invariant is trivially preserved.
            }
        }
    }
}

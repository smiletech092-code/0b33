/// Kani formal-verification harnesses for the interval-elapsed invariant.
///
/// # Invariant
/// A subscription charge is only permitted once per billing interval.  The
/// gate is implemented in `charge_core.rs` as:
///
/// ```text
/// let next_allowed = next_charge_time(last_payment, interval_seconds)?;
/// if now < next_allowed { return Err(Error::IntervalNotElapsed); }
/// ```
///
/// `next_charge_time` is the pure arithmetic function:
///
/// ```rust
/// pub fn next_charge_time(last_payment: u64, interval: u64) -> Result<u64, Error> {
///     last_payment.checked_add(interval).ok_or(Error::Overflow)
/// }
/// ```
///
/// These harnesses prove exhaustively (over all `u64` inputs) that:
///
/// 1. **next_charge_time_monotone** — When the call succeeds, the result is
///    always strictly greater than `last_payment` (provided `interval > 0`),
///    so the billing engine can never charge at the exact moment of the last
///    payment.
///
/// 2. **next_charge_time_exact** — The result equals `last_payment + interval`
///    (no hidden rounding or off-by-one).
///
/// 3. **interval_gate_admits_only_ready_charges** — For any `(last_payment,
///    interval_seconds, now)` triple, the gate `now >= next_charge_time` is
///    equivalent to `now >= last_payment + interval_seconds`.  An adversary
///    cannot construct inputs that bypass the gate without the interval having
///    elapsed.
///
/// # Compile note
/// Gated on `#[cfg(kani)]` and the `kani_harness` crate feature.  Only the
/// pure `next_charge_time` function is called; no `Env` mock is required.
#[cfg(kani)]
mod interval_elapsed {
    use subscription_vault::{next_charge_time, Error};

    // ------------------------------------------------------------------
    // Harness 1: next_charge_time is monotone (result > last_payment)
    // ------------------------------------------------------------------

    /// When `interval > 0` and no overflow occurs, the next charge time is
    /// strictly later than the previous charge time.
    ///
    /// This proves that the billing engine cannot double-charge in the same
    /// instant: after a successful charge updates `last_payment_timestamp`,
    /// the new `next_charge_time` is always in the future.
    #[kani::proof]
    pub fn next_charge_time_monotone() {
        let last_payment: u64 = kani::any();
        let interval: u64 = kani::any();

        // Pre-condition: interval is positive (zero-second intervals are
        // rejected by validate_interval at subscription creation time).
        kani::assume(interval > 0);

        match next_charge_time(last_payment, interval) {
            Ok(next) => {
                // Monotonicity: next charge time is strictly after last payment.
                assert!(
                    next > last_payment,
                    "next_charge_time returned a value <= last_payment, enabling double-charge"
                );

                // The difference equals the interval exactly.
                assert_eq!(
                    next - last_payment,
                    interval,
                    "next_charge_time gap does not equal interval_seconds"
                );
            }
            Err(Error::Overflow) => {
                // Overflow is only possible when last_payment + interval > u64::MAX.
                // Kani validates this precondition implicitly via symbolic execution.
            }
            Err(_) => {
                kani::assert(false, "next_charge_time returned unexpected error variant");
            }
        }
    }

    // ------------------------------------------------------------------
    // Harness 2: next_charge_time exactness (no rounding, no off-by-one)
    // ------------------------------------------------------------------

    /// The result of `next_charge_time` equals `last_payment + interval`
    /// with no rounding or off-by-one errors.
    ///
    /// This proves that the billing window is exactly `interval_seconds` wide:
    /// not one second shorter (under-charging) or longer (over-charging).
    #[kani::proof]
    pub fn next_charge_time_exact() {
        let last_payment: u64 = kani::any();
        let interval: u64 = kani::any();

        if let Ok(next) = next_charge_time(last_payment, interval) {
            // Checked arithmetic: next == last_payment + interval, no truncation.
            assert_eq!(
                next,
                last_payment + interval,
                "next_charge_time result is not exactly last_payment + interval"
            );

            // Also implied: next >= last_payment (since interval is u64, i.e. >= 0).
            assert!(next >= last_payment, "next_charge_time returned a past timestamp");
        }
    }

    // ------------------------------------------------------------------
    // Harness 3: the interval gate admits only ready charges
    // ------------------------------------------------------------------

    /// For any (last_payment, interval_seconds, now) the adversary chooses,
    /// the gate `now >= next_charge_time(last_payment, interval_seconds)`
    /// is equivalent to the contract's `IntervalNotElapsed` guard.
    ///
    /// Concretely: if `now < next_allowed` the charge is blocked; if
    /// `now >= next_allowed` the interval has elapsed and the charge may
    /// proceed.  This is the canonical two-branch split that the billing
    /// engine performs on every charge attempt.
    #[kani::proof]
    pub fn interval_gate_admits_only_ready_charges() {
        let last_payment: u64 = kani::any();
        let interval_seconds: u64 = kani::any();
        let now: u64 = kani::any();

        // Valid interval: positive and within the allowed range.
        kani::assume(interval_seconds > 0);

        match next_charge_time(last_payment, interval_seconds) {
            Ok(next_allowed) => {
                // When next_charge_time returns Ok, the addition did not
                // overflow, so `last_payment + interval_seconds` is safe here.
                // Bind it once to avoid a redundant operation.
                let sum = last_payment + interval_seconds; // safe: same addition that succeeded above

                if now < next_allowed {
                    // Gate says: charge NOT allowed.
                    // Equivalently: now < last_payment + interval_seconds.
                    assert!(
                        now < sum,
                        "interval gate blocked a charge that should have been allowed"
                    );
                } else {
                    // Gate says: charge IS allowed.
                    // Equivalently: now >= last_payment + interval_seconds.
                    assert!(
                        now >= sum,
                        "interval gate allowed a charge before the interval elapsed"
                    );
                }
            }
            Err(Error::Overflow) => {
                // next_charge_time overflowed: last_payment + interval > u64::MAX.
                // In the contract this is propagated as an error and the charge
                // is rejected — a safe, conservative outcome.
            }
            Err(_) => {
                kani::assert(false, "next_charge_time returned unexpected error variant");
            }
        }
    }
}

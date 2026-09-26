/// Kani formal-verification harnesses for the authorization-enforcement invariant.
///
/// # Invariant
/// Privileged operations (batch charge, bulk pause/cancel, emergency stop) must
/// only be callable by the stored admin address or the stored operator address.
/// Any other caller must receive `Error::Unauthorized`.
///
/// The contract enforces this via `require_admin_or_operator_auth`, whose core
/// identity-comparison logic is:
///
/// ```text
/// if caller == stored_admin  { return Ok(());  }
/// if caller == stored_op     { return Ok(());  }
/// return Err(Error::Unauthorized);
/// ```
///
/// Soroban's `Address::require_auth()` is the on-chain layer that ensures the
/// transaction is signed by `caller`; the identity comparison is the second
/// layer that checks the *role* of that signer.  These harnesses verify the
/// second layer — the pure identity logic — exhaustively.
///
/// # What is modeled
/// Because Kani cannot execute Soroban `Env` methods, the harnesses extract
/// the pure decision function (`is_authorized`) and prove three properties:
///
/// 1. **admin_is_always_authorized** — When `caller == stored_admin` the
///    function must return `true`, regardless of whether an operator exists.
///
/// 2. **operator_is_always_authorized** — When `caller == stored_operator`
///    (and the operator slot is populated) the function must return `true`.
///
/// 3. **unauthorized_caller_is_always_rejected** — When `caller` equals
///    neither `stored_admin` nor `stored_operator`, the function must return
///    `false` (which the contract maps to `Err(Error::Unauthorized)`).
///
/// The function under test is the pure Rust decision kernel extracted inline
/// so no Soroban mock is needed.
///
/// # Compile note
/// Gated on `#[cfg(kani)]` and the `kani_harness` crate feature.
#[cfg(kani)]
mod auth_enforcement {
    use subscription_vault::Error;

    // ------------------------------------------------------------------
    // Pure authorization decision function (extracted from admin.rs)
    // ------------------------------------------------------------------
    //
    // This mirrors `require_admin_or_operator_auth` with the `Env` calls
    // removed.  Only the identity-comparison logic is kept so Kani can
    // reason about it symbolically.
    //
    // Postcondition: returns Ok(()) iff caller is admin OR (operator is Some
    // and caller is operator).  Returns Err(Unauthorized) otherwise.
    fn check_auth(
        caller: u64,
        stored_admin: u64,
        stored_operator: Option<u64>,
    ) -> Result<(), Error> {
        if caller == stored_admin {
            return Ok(());
        }
        if let Some(op) = stored_operator {
            if caller == op {
                return Ok(());
            }
        }
        Err(Error::Unauthorized)
    }

    // ------------------------------------------------------------------
    // Harness 1: admin is always authorized
    // ------------------------------------------------------------------

    /// For every symbolic `(caller, stored_admin, stored_operator)` triple
    /// where `caller == stored_admin`, `check_auth` must return `Ok(())`.
    ///
    /// This proves that a transaction signed by the stored admin address is
    /// **always** admitted, regardless of the operator slot state.
    #[kani::proof]
    pub fn admin_is_always_authorized() {
        let caller: u64 = kani::any();
        let stored_admin: u64 = kani::any();
        let stored_operator: Option<u64> = kani::any();

        // Pre-condition: the caller is the admin.
        kani::assume(caller == stored_admin);

        let result = check_auth(caller, stored_admin, stored_operator);

        assert!(
            result.is_ok(),
            "admin caller was incorrectly rejected — auth gate is broken"
        );
    }

    // ------------------------------------------------------------------
    // Harness 2: operator is always authorized (when slot is populated)
    // ------------------------------------------------------------------

    /// For every symbolic triple where `stored_operator == Some(op)` and
    /// `caller == op`, `check_auth` must return `Ok(())`.
    ///
    /// This proves that the operator role grants the intended access to
    /// privileged bulk operations, independently of the admin address.
    #[kani::proof]
    pub fn operator_is_always_authorized() {
        let caller: u64 = kani::any();
        let stored_admin: u64 = kani::any();
        let op: u64 = kani::any();

        // Pre-conditions:
        // - Operator slot is populated.
        // - The caller is the operator.
        // - Operator is distinct from admin (the common case; the same-address
        //   case is covered by admin_is_always_authorized).
        kani::assume(caller == op);
        kani::assume(caller != stored_admin);

        let result = check_auth(caller, stored_admin, Some(op));

        assert!(
            result.is_ok(),
            "operator caller was incorrectly rejected — auth gate is broken"
        );
    }

    // ------------------------------------------------------------------
    // Harness 3: unauthorized callers are always rejected
    // ------------------------------------------------------------------

    /// For every symbolic triple where `caller` is neither `stored_admin` nor
    /// `stored_operator`, `check_auth` must return `Err(Error::Unauthorized)`.
    ///
    /// This proves **no bypass exists**: an adversary who controls all inputs
    /// but whose address is not registered as admin or operator will always be
    /// rejected.  This is the strongest form of the auth invariant.
    #[kani::proof]
    pub fn unauthorized_caller_is_always_rejected() {
        let caller: u64 = kani::any();
        let stored_admin: u64 = kani::any();
        let stored_operator: Option<u64> = kani::any();

        // Pre-condition: the caller is neither admin nor operator.
        kani::assume(caller != stored_admin);
        if let Some(op) = stored_operator {
            kani::assume(caller != op);
        }

        let result = check_auth(caller, stored_admin, stored_operator);

        assert!(
            result.is_err(),
            "unauthorized caller was incorrectly admitted — critical auth bypass"
        );

        // Also check the specific error code so callers get the right signal.
        assert_eq!(
            result.unwrap_err(),
            Error::Unauthorized,
            "wrong error code returned for unauthorized caller"
        );
    }

    // ------------------------------------------------------------------
    // Harness 4: exhaustive bifurcation — Ok and Err are mutually exclusive
    // ------------------------------------------------------------------

    /// The two outcomes (Ok and Err) are mutually exclusive and exhaustive.
    /// Kani will find a counterexample if any input leads to an impossible
    /// third outcome (e.g., panic or wrong error variant).
    ///
    /// This is the "total correctness" closure over the auth function.
    #[kani::proof]
    pub fn auth_result_is_binary() {
        let caller: u64 = kani::any();
        let stored_admin: u64 = kani::any();
        let stored_operator: Option<u64> = kani::any();

        let is_authorized = caller == stored_admin
            || stored_operator.map_or(false, |op| caller == op);

        match check_auth(caller, stored_admin, stored_operator) {
            Ok(()) => {
                assert!(
                    is_authorized,
                    "check_auth returned Ok for a caller that should be unauthorized"
                );
            }
            Err(Error::Unauthorized) => {
                assert!(
                    !is_authorized,
                    "check_auth returned Unauthorized for a caller that should be authorized"
                );
            }
            Err(e) => {
                // Any error other than Unauthorized is a logic bug.
                let _ = e;
                kani::assert(
                    false,
                    "check_auth returned an unexpected error variant (not Unauthorized)",
                );
            }
        }
    }
}

//! Replay Domain Isolation & Nonce Security Tests
//!
//! This module verifies that monotonic nonce counters used across distinct operational domains
//! do not collide or cross-consume. Specifically, it tests isolation between:
//! - `DOMAIN_CHARGE_INTERVAL` (4)
//! - `DOMAIN_DEPOSIT_FUNDS` (5)
//! - `DOMAIN_CHARGE_ONEOFF` (6)
//!
//! # Security Notes
//! - **Domain Separation**: In Soroban storage, nonces are indexed by `DataKey::AdminNonce(signer, domain)`.
//!   Because `domain` is part of the persistent storage key, operators and subscribers can share the same
//!   counter sequence across different transaction types without risk of cross-domain replay attacks or
//!   denial-of-service via counter exhaustion.
//! - **Per-Signer Isolation**: Each `(Address, u32)` tuple maintains an independent counter. An operator
//!   advancing a nonce for Subscription A cannot affect the nonce counter for Subscription B or any other signer.
//! - **Overflow Protection**: Checked arithmetic prevents wrapping at `u64::MAX`. Attempting to consume `u64::MAX`
//!   returns `Error::Overflow` rather than wrapping to zero.
//! - **Authentication Order**: In production contract methods, authentication (`require_admin_auth` or signature verification)
//!   MUST occur before nonce checking to prevent unauthenticated callers from probing or advancing nonce counters.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};
use crate::nonce::{
    check_and_advance, compute_next_nonce, get_nonce,
    DOMAIN_BATCH_CHARGE, DOMAIN_ADMIN_ROTATION, DOMAIN_OPERATOR_BATCH_CHARGE,
    DOMAIN_METADATA_SIGNED, DOMAIN_CHARGE_INTERVAL, DOMAIN_DEPOSIT_FUNDS,
    DOMAIN_CHARGE_ONEOFF, DOMAIN_MERCHANT_ROTATION, DOMAIN_SUBSCRIBER_WITHDRAWAL,
    DOMAIN_CHARGEBACK_DISPUTE, DOMAIN_GOVERNANCE_VOTE,
};
use crate::types::{DataKey, Error};

/// Verifies core domain isolation as specified in issue #603:
/// 1. Consume nonce N under domain A.
/// 2. Consume nonce N under domain B (must succeed).
/// 3. Re-consume N under domain A (must reject).
#[test]
fn test_nonce_domain_isolation_core() {
    let env = Env::default();
    let signer = Address::generate(&env);
    let contract_id = env.register(crate::SubscriptionVault, ());

    env.as_contract(&contract_id, || {
        let domain_a = DOMAIN_CHARGE_INTERVAL;
        let domain_b = DOMAIN_DEPOSIT_FUNDS;
        let domain_c = DOMAIN_CHARGE_ONEOFF;
        let nonce_n = 0u64;

        // Verify initial state across all domains is 0
        assert_eq!(get_nonce(&env, &signer, domain_a), nonce_n);
        assert_eq!(get_nonce(&env, &signer, domain_b), nonce_n);
        assert_eq!(get_nonce(&env, &signer, domain_c), nonce_n);

        // 1. Consume nonce N under domain A
        assert_eq!(check_and_advance(&env, &signer, domain_a, nonce_n), Ok(()));
        assert_eq!(get_nonce(&env, &signer, domain_a), nonce_n + 1);
        // Ensure domain B and C remain untouched
        assert_eq!(get_nonce(&env, &signer, domain_b), nonce_n);
        assert_eq!(get_nonce(&env, &signer, domain_c), nonce_n);

        // 2. Consume nonce N under domain B (must succeed despite N being consumed in domain A)
        assert_eq!(check_and_advance(&env, &signer, domain_b, nonce_n), Ok(()));
        assert_eq!(get_nonce(&env, &signer, domain_b), nonce_n + 1);
        assert_eq!(get_nonce(&env, &signer, domain_c), nonce_n);

        // 3. Re-consume N under domain A (must reject as domain A is now at N+1)
        assert_eq!(
            check_and_advance(&env, &signer, domain_a, nonce_n),
            Err(Error::NonceAlreadyUsed)
        );
        // Ensure failed re-consumption did not advance the counter
        assert_eq!(get_nonce(&env, &signer, domain_a), nonce_n + 1);

        // Verify we can still consume nonce N under domain C without collision
        assert_eq!(check_and_advance(&env, &signer, domain_c, nonce_n), Ok(()));
        assert_eq!(get_nonce(&env, &signer, domain_c), nonce_n + 1);
    });
}

/// Edge case test: Cross-subscription / cross-signer isolation for the same nonce and domain.
/// Verifies that two different operators/subscribers using the exact same nonce under the exact same
/// replay domain do not collide or interfere with each other's execution flow.
#[test]
fn test_cross_subscription_same_nonce() {
    let env = Env::default();
    let subscriber1 = Address::generate(&env);
    let subscriber2 = Address::generate(&env);
    let contract_id = env.register(crate::SubscriptionVault, ());

    env.as_contract(&contract_id, || {
        let domain = DOMAIN_CHARGE_INTERVAL;

        // Both subscribers start at nonce 0
        assert_eq!(get_nonce(&env, &subscriber1, domain), 0);
        assert_eq!(get_nonce(&env, &subscriber2, domain), 0);

        // Subscriber 1 advances nonces 0, 1, 2
        assert_eq!(check_and_advance(&env, &subscriber1, domain, 0), Ok(()));
        assert_eq!(check_and_advance(&env, &subscriber1, domain, 1), Ok(()));
        assert_eq!(check_and_advance(&env, &subscriber1, domain, 2), Ok(()));
        assert_eq!(get_nonce(&env, &subscriber1, domain), 3);

        // Subscriber 2 consuming nonce 0 under the same domain MUST succeed
        assert_eq!(check_and_advance(&env, &subscriber2, domain, 0), Ok(()));
        assert_eq!(get_nonce(&env, &subscriber2, domain), 1);

        // Subscriber 2 consuming nonce 1 MUST succeed
        assert_eq!(check_and_advance(&env, &subscriber2, domain, 1), Ok(()));
        assert_eq!(get_nonce(&env, &subscriber2, domain), 2);

        // Verify Subscriber 1's nonce counter was not altered
        assert_eq!(get_nonce(&env, &subscriber1, domain), 3);
    });
}

/// Edge case test: Nonce zero consumption and initialization across all defined domains.
/// Verifies that nonce 0 is uniformly accepted as the starting state and increments cleanly to 1.
#[test]
fn test_nonce_zero_consumption_all_domains() {
    let env = Env::default();
    let signer = Address::generate(&env);
    let contract_id = env.register(crate::SubscriptionVault, ());

    let all_domains = [
        DOMAIN_BATCH_CHARGE,
        DOMAIN_ADMIN_ROTATION,
        DOMAIN_OPERATOR_BATCH_CHARGE,
        DOMAIN_METADATA_SIGNED,
        DOMAIN_CHARGE_INTERVAL,
        DOMAIN_DEPOSIT_FUNDS,
        DOMAIN_CHARGE_ONEOFF,
        DOMAIN_MERCHANT_ROTATION,
        DOMAIN_SUBSCRIBER_WITHDRAWAL,
        DOMAIN_CHARGEBACK_DISPUTE,
        DOMAIN_GOVERNANCE_VOTE,
    ];

    env.as_contract(&contract_id, || {
        for domain in all_domains {
            // Initial nonce must be 0
            assert_eq!(get_nonce(&env, &signer, domain), 0);

            // Consuming nonce 0 must succeed
            assert_eq!(check_and_advance(&env, &signer, domain, 0), Ok(()));
            assert_eq!(get_nonce(&env, &signer, domain), 1);

            // Re-consuming nonce 0 must fail
            assert_eq!(
                check_and_advance(&env, &signer, domain, 0),
                Err(Error::NonceAlreadyUsed)
            );
        }
    });
}

/// Edge case test: Nonce overflow at `u64::MAX`.
/// Verifies that when a nonce reaches `u64::MAX`, any attempt to consume it is rejected with `Error::Overflow`
/// rather than wrapping to zero and reopening replay vulnerabilities.
#[test]
fn test_nonce_max_overflow_domain_isolation() {
    let env = Env::default();
    let signer = Address::generate(&env);
    let contract_id = env.register(crate::SubscriptionVault, ());

    env.as_contract(&contract_id, || {
        let domain = DOMAIN_CHARGE_ONEOFF;
        let key = DataKey::AdminNonce(signer.clone(), domain.as_u32());

        // Artificially seed storage with u64::MAX
        env.storage().persistent().set(&key, &u64::MAX);
        assert_eq!(get_nonce(&env, &signer, domain), u64::MAX);

        // Attempting to advance u64::MAX must return Error::Overflow
        assert_eq!(
            check_and_advance(&env, &signer, domain, u64::MAX),
            Err(Error::Overflow)
        );

        // Verify counter did not wrap around to 0
        assert_eq!(get_nonce(&env, &signer, domain), u64::MAX);

        // Verify pure helper math rejects overflow
        assert_eq!(compute_next_nonce(u64::MAX, u64::MAX), Err(Error::Overflow));
    });
}

/// Verifies total independence across all 11 domain constants simultaneously.
#[test]
fn test_all_domains_mutual_independence() {
    let env = Env::default();
    let signer = Address::generate(&env);
    let contract_id = env.register(crate::SubscriptionVault, ());

    let all_domains = [
        DOMAIN_BATCH_CHARGE,
        DOMAIN_ADMIN_ROTATION,
        DOMAIN_OPERATOR_BATCH_CHARGE,
        DOMAIN_METADATA_SIGNED,
        DOMAIN_CHARGE_INTERVAL,
        DOMAIN_DEPOSIT_FUNDS,
        DOMAIN_CHARGE_ONEOFF,
        DOMAIN_MERCHANT_ROTATION,
        DOMAIN_SUBSCRIBER_WITHDRAWAL,
        DOMAIN_CHARGEBACK_DISPUTE,
        DOMAIN_GOVERNANCE_VOTE,
    ];

    env.as_contract(&contract_id, || {
        // Step 1: Advance each domain by a different number of steps (domain index + 1 times)
        for (idx, &domain) in all_domains.iter().enumerate() {
            for step in 0..=(idx as u64) {
                assert_eq!(check_and_advance(&env, &signer, domain, step), Ok(()));
            }
        }

        // Step 2: Verify each domain has its exact expected counter value (idx + 1)
        for (idx, &domain) in all_domains.iter().enumerate() {
            assert_eq!(get_nonce(&env, &signer, domain), (idx as u64) + 1);
        }
    });
}

/// Verifies that deposit_funds nonce domain separation prevents replay attacks.
/// Acceptance criteria: deposit_funds accepts an optional nonce key; replay returns Replay (code 4005).
/// 
/// This test ensures:
/// 1. A valid nonce (0) can be consumed successfully for a deposit_funds call
/// 2. Attempting to replay the same nonce (0) returns Error::Replay (code 4005)
/// 3. The nonce counter is independent from other domains and signers
#[test]
fn test_deposit_funds_nonce_replay() {
    let env = Env::default();
    let subscriber = Address::generate(&env);
    let contract_id = env.register(crate::SubscriptionVault, ());

    env.as_contract(&contract_id, || {
        let domain = DOMAIN_DEPOSIT_FUNDS;
        let nonce_0 = 0u64;

        // Initial state: nonce should be 0 (not consumed)
        assert_eq!(get_nonce(&env, &subscriber, domain), nonce_0);

        // First consumption: nonce 0 should succeed
        assert_eq!(check_and_advance(&env, &subscriber, domain, nonce_0), Ok(()));
        assert_eq!(get_nonce(&env, &subscriber, domain), 1);

        // Replay attempt: try to consume nonce 0 again (should fail with NonceAlreadyUsed)
        assert_eq!(
            check_and_advance(&env, &subscriber, domain, nonce_0),
            Err(Error::NonceAlreadyUsed)
        );

        // Verify counter was not incremented by the failed replay
        assert_eq!(get_nonce(&env, &subscriber, domain), 1);

        // Valid next nonce: nonce 1 should succeed
        assert_eq!(check_and_advance(&env, &subscriber, domain, 1), Ok(()));
        assert_eq!(get_nonce(&env, &subscriber, domain), 2);

        // Second replay attempt: try to consume nonce 0 again (must still fail)
        assert_eq!(
            check_and_advance(&env, &subscriber, domain, nonce_0),
            Err(Error::NonceAlreadyUsed)
        );

        // Verify it works independently for a different signer
        let other_subscriber = Address::generate(&env);
        assert_eq!(get_nonce(&env, &other_subscriber, domain), nonce_0);
        assert_eq!(check_and_advance(&env, &other_subscriber, domain, nonce_0), Ok(()));
        assert_eq!(get_nonce(&env, &other_subscriber, domain), 1);

        // First subscriber's counter should remain unchanged
        assert_eq!(get_nonce(&env, &subscriber, domain), 2);
    });
}


/// Verifies that DOMAIN_GOVERNANCE_VOTE nonces prevent double-vote replay attacks.
///
/// A node that retransmits a signed vote transaction must be rejected after
/// the first successful consumption. This test directly exercises the nonce
/// layer; see `test_governance_vote_nonce_domain_isolation` for cross-domain
/// non-interference.
#[test]
fn test_governance_vote_nonce_replay() {
    let env = Env::default();
    let guardian = Address::generate(&env);
    let contract_id = env.register(crate::SubscriptionVault, ());

    env.as_contract(&contract_id, || {
        let domain = DOMAIN_GOVERNANCE_VOTE;

        // Initial state: nonce is 0
        assert_eq!(get_nonce(&env, &guardian, domain), 0);

        // First vote nonce consumption must succeed
        assert_eq!(check_and_advance(&env, &guardian, domain, 0), Ok(()));
        assert_eq!(get_nonce(&env, &guardian, domain), 1);

        // Replay of the same nonce must be rejected
        assert_eq!(
            check_and_advance(&env, &guardian, domain, 0),
            Err(Error::NonceAlreadyUsed)
        );

        // Counter must not advance on failed replay
        assert_eq!(get_nonce(&env, &guardian, domain), 1);

        // Second legitimate vote nonce must succeed
        assert_eq!(check_and_advance(&env, &guardian, domain, 1), Ok(()));
        assert_eq!(get_nonce(&env, &guardian, domain), 2);

        // Another replay attempt for nonce 0 must still fail
        assert_eq!(
            check_and_advance(&env, &guardian, domain, 0),
            Err(Error::NonceAlreadyUsed)
        );
    });
}

/// Verifies DOMAIN_GOVERNANCE_VOTE is fully isolated from all other domains.
///
/// A vote nonce consumed under DOMAIN_GOVERNANCE_VOTE must not affect or be
/// affected by any other domain's counter for the same signer.
#[test]
fn test_governance_vote_nonce_domain_isolation() {
    let env = Env::default();
    let guardian = Address::generate(&env);
    let contract_id = env.register(crate::SubscriptionVault, ());

    env.as_contract(&contract_id, || {
        // Advance DOMAIN_BATCH_CHARGE twice so it sits at nonce 2
        assert_eq!(check_and_advance(&env, &guardian, DOMAIN_BATCH_CHARGE, 0), Ok(()));
        assert_eq!(check_and_advance(&env, &guardian, DOMAIN_BATCH_CHARGE, 1), Ok(()));

        // DOMAIN_GOVERNANCE_VOTE must still start at 0
        assert_eq!(get_nonce(&env, &guardian, DOMAIN_GOVERNANCE_VOTE), 0);

        // Consume governance vote nonce 0 — must succeed despite DOMAIN_BATCH_CHARGE being at 2
        assert_eq!(check_and_advance(&env, &guardian, DOMAIN_GOVERNANCE_VOTE, 0), Ok(()));
        assert_eq!(get_nonce(&env, &guardian, DOMAIN_GOVERNANCE_VOTE), 1);

        // DOMAIN_BATCH_CHARGE counter must remain unaffected
        assert_eq!(get_nonce(&env, &guardian, DOMAIN_BATCH_CHARGE), 2);

        // Replay of governance vote nonce 0 must still fail
        assert_eq!(
            check_and_advance(&env, &guardian, DOMAIN_GOVERNANCE_VOTE, 0),
            Err(Error::NonceAlreadyUsed)
        );

        // All other domains must remain at their initial states
        assert_eq!(get_nonce(&env, &guardian, DOMAIN_ADMIN_ROTATION), 0);
        assert_eq!(get_nonce(&env, &guardian, DOMAIN_OPERATOR_BATCH_CHARGE), 0);
        assert_eq!(get_nonce(&env, &guardian, DOMAIN_METADATA_SIGNED), 0);
        assert_eq!(get_nonce(&env, &guardian, DOMAIN_MERCHANT_ROTATION), 0);
        assert_eq!(get_nonce(&env, &guardian, DOMAIN_CHARGE_INTERVAL), 0);
        assert_eq!(get_nonce(&env, &guardian, DOMAIN_DEPOSIT_FUNDS), 0);
        assert_eq!(get_nonce(&env, &guardian, DOMAIN_CHARGE_ONEOFF), 0);
        assert_eq!(get_nonce(&env, &guardian, DOMAIN_SUBSCRIBER_WITHDRAWAL), 0);
        assert_eq!(get_nonce(&env, &guardian, DOMAIN_CHARGEBACK_DISPUTE), 0);
    });
}

/// Verifies per-guardian nonce isolation: one guardian's votes cannot advance
/// another guardian's DOMAIN_GOVERNANCE_VOTE counter.
#[test]
fn test_governance_vote_nonce_per_guardian_isolation() {
    let env = Env::default();
    let guardian1 = Address::generate(&env);
    let guardian2 = Address::generate(&env);
    let contract_id = env.register(crate::SubscriptionVault, ());

    env.as_contract(&contract_id, || {
        let domain = DOMAIN_GOVERNANCE_VOTE;

        // Guardian 1 advances its nonce to 3
        assert_eq!(check_and_advance(&env, &guardian1, domain, 0), Ok(()));
        assert_eq!(check_and_advance(&env, &guardian1, domain, 1), Ok(()));
        assert_eq!(check_and_advance(&env, &guardian1, domain, 2), Ok(()));
        assert_eq!(get_nonce(&env, &guardian1, domain), 3);

        // Guardian 2 must still be at 0, independent of guardian 1
        assert_eq!(get_nonce(&env, &guardian2, domain), 0);
        assert_eq!(check_and_advance(&env, &guardian2, domain, 0), Ok(()));
        assert_eq!(get_nonce(&env, &guardian2, domain), 1);

        // Guardian 1 counter must be unchanged by guardian 2's activity
        assert_eq!(get_nonce(&env, &guardian1, domain), 3);
    });
}

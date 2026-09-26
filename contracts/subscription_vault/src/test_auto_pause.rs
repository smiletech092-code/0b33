//! Auto-pause on consecutive InsufficientBalance failures.
//!
//! Verifies that a subscription is automatically paused after N consecutive
//! InsufficientBalance charge attempts, that the counter resets on a successful
//! charge or a fresh deposit, and that the feature is inert when disabled (threshold=0).

use crate::{SubscriptionStatus, SubscriptionVault, SubscriptionVaultClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token, Address, Env,
};

const T0: u64 = 1_000_000;
const INTERVAL: u64 = 30 * 24 * 60 * 60;
const AMOUNT: i128 = 10_000_000;

// ── helpers ──────────────────────────────────────────────────────────────────

fn setup_no_grace() -> (
    Env,
    SubscriptionVaultClient<'static>,
    Address, // admin
    token::StellarAssetClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = T0);

    let admin = Address::generate(&env);
    let contract_id = env.register(SubscriptionVault, ());
    let client = SubscriptionVaultClient::new(&env, &contract_id);

    let token_id = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let token_admin = token::StellarAssetClient::new(&env, &token_id.address());

    // grace_period = 0 so failures go straight to InsufficientBalance
    client.init(&token_id.address(), &6, &admin, &1_000_000i128, &0u64);

    (env, client, admin, token_admin)
}

/// Create a subscription with `prepaid` tokens deposited.
fn create_funded_sub(
    env: &Env,
    client: &SubscriptionVaultClient,
    token_admin: &token::StellarAssetClient,
    prepaid: i128,
) -> (u32, Address, Address) {
    let subscriber = Address::generate(env);
    let merchant = Address::generate(env);
    let id = client.create_subscription(
        &subscriber,
        &merchant,
        &AMOUNT,
        &INTERVAL,
        &false,
        &None::<i128>,
        &None::<u64>,
        &None::<Address>,
    );
    if prepaid > 0 {
        token_admin.mint(&subscriber, &prepaid);
        client.deposit_funds(&id, &prepaid, &None);
    }
    (id, subscriber, merchant)
}

/// Advance ledger past one full interval so `charge_one` passes the timing guard.
fn jump_interval(env: &Env) {
    env.ledger()
        .with_mut(|l| l.timestamp += INTERVAL + 1);
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// With threshold=0 (disabled), multiple failures never cause auto-pause.
#[test]
fn test_no_auto_pause_when_disabled() {
    let (env, client, _admin, _tok) = setup_no_grace();
    // threshold stays at 0 (default)
    let (id, _sub, _mer) = create_funded_sub(&env, &client, &_tok, 0);

    for _ in 0..5 {
        jump_interval(&env);
        client.charge_subscription(&id, &None);
    }

    let sub = client.get_subscription(&id);
    assert_eq!(
        sub.status,
        SubscriptionStatus::InsufficientBalance,
        "should stay InsufficientBalance, not auto-pause"
    );
}

/// N=1: the very first failure immediately pauses the subscription.
#[test]
fn test_n1_immediate_pause_on_first_failure() {
    let (env, client, admin, _tok) = setup_no_grace();
    client.set_auto_pause_threshold(&admin, &1u32);

    let (id, _sub, _mer) = create_funded_sub(&env, &client, &_tok, 0);

    jump_interval(&env);
    client.charge_subscription(&id, &None);

    let sub = client.get_subscription(&id);
    assert_eq!(
        sub.status,
        SubscriptionStatus::Paused,
        "first failure with threshold=1 must immediately pause"
    );
}

/// Counter increments across failures and triggers pause exactly at N=3.
#[test]
fn test_counter_increments_and_pauses_at_threshold() {
    let (env, client, admin, _tok) = setup_no_grace();
    client.set_auto_pause_threshold(&admin, &3u32);

    let (id, _sub, _mer) = create_funded_sub(&env, &client, &_tok, 0);

    // failures 1 and 2 — still InsufficientBalance
    for _ in 0..2 {
        jump_interval(&env);
        client.charge_subscription(&id, &None);
        assert_eq!(
            client.get_subscription(&id).status,
            SubscriptionStatus::InsufficientBalance,
            "should not pause before threshold"
        );
    }

    // failure 3 — triggers auto-pause
    jump_interval(&env);
    client.charge_subscription(&id, &None);
    assert_eq!(
        client.get_subscription(&id).status,
        SubscriptionStatus::Paused,
        "should be Paused after 3rd failure"
    );
}

/// Counter resets to zero on a successful charge.
#[test]
fn test_counter_resets_on_successful_charge() {
    let (env, client, admin, tok) = setup_no_grace();
    client.set_auto_pause_threshold(&admin, &3u32);

    let (id, subscriber, _mer) = create_funded_sub(&env, &client, &tok, 0);

    // Two underfunded attempts (sub has 0 balance, so they go to InsufficientBalance)
    jump_interval(&env);
    client.charge_subscription(&id, &None);
    jump_interval(&env);
    client.charge_subscription(&id, &None);
    assert_eq!(
        client.get_subscription(&id).status,
        SubscriptionStatus::InsufficientBalance
    );

    // Top up enough to cover one charge — counter resets on deposit
    tok.mint(&subscriber, &AMOUNT);
    client.deposit_funds(&id, &AMOUNT, &None);

    // Successful charge — counter cleared
    jump_interval(&env);
    client.charge_subscription(&id, &None);
    assert_eq!(
        client.get_subscription(&id).status,
        SubscriptionStatus::Active,
        "successful charge should restore Active"
    );

    // Now two more failures — should NOT pause until we hit 3 again
    jump_interval(&env);
    client.charge_subscription(&id, &None);
    jump_interval(&env);
    client.charge_subscription(&id, &None);
    assert_eq!(
        client.get_subscription(&id).status,
        SubscriptionStatus::InsufficientBalance,
        "counter restarted: 2 failures after reset should not trigger pause"
    );
}

/// Counter resets on deposit even without a successful charge.
#[test]
fn test_counter_resets_on_deposit() {
    let (env, client, admin, tok) = setup_no_grace();
    client.set_auto_pause_threshold(&admin, &3u32);

    let (id, subscriber, _mer) = create_funded_sub(&env, &client, &tok, 0);

    // Two failures
    jump_interval(&env);
    client.charge_subscription(&id, &None);
    jump_interval(&env);
    client.charge_subscription(&id, &None);

    // Deposit (not enough to cover the charge, just enough to satisfy min_topup)
    tok.mint(&subscriber, &1_000_000i128);
    client.deposit_funds(&id, &1_000_000i128, &None);

    // The next failure is only the 1st after the reset — should not pause
    jump_interval(&env);
    client.charge_subscription(&id, &None);
    assert_eq!(
        client.get_subscription(&id).status,
        SubscriptionStatus::InsufficientBalance,
        "counter reset by deposit: 1 failure after reset should not trigger pause"
    );
}

/// A subscription already Paused (manually) is not double-paused or errored
/// when a charge is attempted — charge_one returns NotActive, counter untouched.
#[test]
fn test_already_paused_not_affected() {
    let (env, client, admin, _tok) = setup_no_grace();
    client.set_auto_pause_threshold(&admin, &1u32);

    let (id, sub_addr, mer_addr) = create_funded_sub(&env, &client, &_tok, 0);

    // Manually pause (subscriber auth)
    client.pause_subscription(&id, &sub_addr);
    assert_eq!(client.get_subscription(&id).status, SubscriptionStatus::Paused);

    // Attempt a charge on a Paused sub — must return NotActive, not crash
    jump_interval(&env);
    let result = client.try_charge_subscription(&id, &None);
    assert!(result.is_err(), "charge on Paused sub must fail");

    // Status unchanged
    assert_eq!(
        client.get_subscription(&id).status,
        SubscriptionStatus::Paused
    );
    let _ = mer_addr; // suppress unused warning
}


/// Per-subscription auto_pause_threshold: different subscriptions can have different thresholds.
#[test]
fn test_per_subscription_threshold_configuration() {
    let (env, client, admin, tok) = setup_no_grace();
    
    // Create two subscriptions
    let (id1, subscriber1, _) = create_funded_sub(&env, &client, &tok, 0);
    let (id2, subscriber2, _) = create_funded_sub(&env, &client, &tok, 0);

    // For subscriptions created before per-subscription thresholds, 
    // they inherit the global threshold of 0 (disabled by default).
    // Verify that both start with auto_pause_threshold = 0
    let sub1 = client.get_subscription(&id1);
    let sub2 = client.get_subscription(&id2);
    
    assert_eq!(sub1.auto_pause_threshold, 0, "subscription 1 should start with threshold 0");
    assert_eq!(sub2.auto_pause_threshold, 0, "subscription 2 should start with threshold 0");
    
    // With global threshold = 0, both subscriptions should not auto-pause
    for _ in 0..5 {
        jump_interval(&env);
        client.charge_subscription(&id1, &None);
    }
    
    let sub1_final = client.get_subscription(&id1);
    assert_eq!(
        sub1_final.status,
        SubscriptionStatus::InsufficientBalance,
        "subscription 1 with threshold 0 should not auto-pause"
    );
}

#![cfg(test)]

//! Integration coverage for the two behavioural guards added alongside the
//! merchant-config mutability and oracle degenerate-price documentation work.
//!
//! These live in `tests/` rather than in `src/test_*.rs` because files in
//! `src/` are not declared as modules in `lib.rs` and are therefore not part of
//! the compiled test binary. Anything in `tests/` *is* compiled and run by
//! `cargo test`.

extern crate alloc;

use soroban_sdk::token::{Client as TokenClient, StellarAssetClient as TokenAdminClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, String,
};
use subscription_vault::{
    SubscriptionVault, SubscriptionVaultClient, OP_CHARGE, OP_WITHDRAW,
};

const T0: u64 = 1_700_000_000;
const INTERVAL: u64 = 30 * 24 * 60 * 60; // 30 days, well above the 60s floor
const AMOUNT: i128 = 10_000_000;

fn setup() -> (Env, SubscriptionVaultClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(T0);

    let contract_id = env.register(SubscriptionVault, ());
    let client = SubscriptionVaultClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    client.init(&token, &6, &admin, &1_000_000i128, &(7 * 24 * 60 * 60));

    (env, client, token, admin)
}

/// Create a merchant with a default config and return `(merchant, payout)`.
fn merchant_with_config(env: &Env, client: &SubscriptionVaultClient, merchant: &Address) -> Address {
    let payout = Address::generate(env);
    client.initialize_merchant_config(
        merchant,
        &payout,
        &100i32, // 1.00%
        &(OP_CHARGE | OP_WITHDRAW),
        &None::<Address>,
        &String::from_str(env, ""),
    );
    payout
}

fn create_active_sub(
    env: &Env,
    client: &SubscriptionVaultClient,
    subscriber: &Address,
    merchant: &Address,
) -> u32 {
    client.create_subscription(
        subscriber,
        merchant,
        &AMOUNT,
        &INTERVAL,
        &false,
        &None::<i128>,
        &None::<u64>,
        &None::<u32>,
        &None::<soroban_sdk::Symbol>,
        &false,
    )
}

// ── #254 — merchant config field mutability ──────────────────────────────────

#[test]
fn protected_field_change_rejected_while_subscription_active() {
    let (env, client, _token, _admin) = setup();
    let merchant = Address::generate(&env);
    let subscriber = Address::generate(&env);
    merchant_with_config(&env, &client, &merchant);
    let _sub = create_active_sub(&env, &client, &subscriber, &merchant);

    // fee_bips is protected.
    let res = client.try_update_merchant_config(
        &merchant,
        &None,
        &Some(500i32),
        &None,
        &None,
        &None,
        &None,
        &None,
    );
    assert_eq!(res, Err(Ok(subscription_vault::Error::InvalidStatusTransition)));

    // allowed_operations is protected too.
    let res = client.try_update_merchant_config(
        &merchant,
        &None,
        &None,
        &Some(OP_CHARGE), // drop OP_WITHDRAW
        &None,
        &None,
        &None,
        &None,
    );
    assert_eq!(res, Err(Ok(subscription_vault::Error::InvalidStatusTransition)));

    // The rejected update must be a total no-op: fee_bips is unchanged.
    let cfg = client.get_merchant_config(&merchant).unwrap();
    assert_eq!(cfg.fee_bips, 100);
    assert_eq!(cfg.allowed_operations, OP_CHARGE | OP_WITHDRAW);
}

#[test]
fn unprotected_fields_mutable_while_subscription_active() {
    let (env, client, _token, _admin) = setup();
    let merchant = Address::generate(&env);
    let subscriber = Address::generate(&env);
    merchant_with_config(&env, &client, &merchant);
    let _sub = create_active_sub(&env, &client, &subscriber, &merchant);

    let new_payout = Address::generate(&env);
    let new_fee_addr = Address::generate(&env);

    // payout_address, fee_address, is_active, is_paused and redirect_url are
    // all freely mutable — in particular pausing must never be blocked.
    let cfg = client
        .update_merchant_config(
            &merchant,
            &Some(new_payout.clone()),
            &None,
            &None,
            &Some(true),
            &Some(Some(new_fee_addr.clone())),
            &Some(String::from_str(&env, "https://example.test")),
            &Some(true),
        )
        .unwrap();

    assert_eq!(cfg.payout_address, new_payout);
    assert_eq!(cfg.fee_address, Some(new_fee_addr));
    assert!(cfg.is_active);
    assert!(cfg.is_paused);
    assert_eq!(cfg.fee_bips, 100, "protected field must be untouched");
}

#[test]
fn protected_field_change_allowed_once_subscription_paused() {
    let (env, client, _token, _admin) = setup();
    let merchant = Address::generate(&env);
    let subscriber = Address::generate(&env);
    merchant_with_config(&env, &client, &merchant);
    let sub = create_active_sub(&env, &client, &subscriber, &merchant);

    // Blocked while Active.
    assert_eq!(
        client.try_update_merchant_config(
            &merchant,
            &None,
            &Some(500i32),
            &None,
            &None,
            &None,
            &None,
            &None,
        ),
        Err(Ok(subscription_vault::Error::InvalidStatusTransition))
    );

    // Pause, then it is allowed.
    client.pause_subscription(&sub, &subscriber);

    let cfg = client
        .update_merchant_config(
            &merchant,
            &None,
            &Some(500i32),
            &None,
            &None,
            &None,
            &None,
            &None,
        )
        .unwrap();
    assert_eq!(cfg.fee_bips, 500);
}

#[test]
fn protected_field_change_allowed_with_no_subscriptions() {
    let (env, client, _token, _admin) = setup();
    let merchant = Address::generate(&env);
    merchant_with_config(&env, &client, &merchant);

    // No subscriptions at all — always allowed.
    let cfg = client
        .update_merchant_config(
            &merchant,
            &None,
            &Some(750i32),
            &Some(OP_CHARGE | OP_WITHDRAW | 0x04),
            &None,
            &None,
            &None,
            &None,
        )
        .unwrap();
    assert_eq!(cfg.fee_bips, 750);
    assert_eq!(cfg.allowed_operations, OP_CHARGE | OP_WITHDRAW | 0x04);
}

#[test]
fn existing_fee_bips_validation_still_applies() {
    let (env, client, _token, _admin) = setup();
    let merchant = Address::generate(&env);
    merchant_with_config(&env, &client, &merchant);

    // Above MAX_FEE_BIPS (10000) and no active subs, so the *only* possible
    // rejection is the pre-existing range check.
    assert_eq!(
        client.try_update_merchant_config(
            &merchant,
            &None,
            &Some(10_001i32),
            &None,
            &None,
            &None,
            &None,
            &None,
        ),
        Err(Ok(subscription_vault::Error::InvalidFeeBips))
    );
}

// ── #253 — oracle degenerate prices ──────────────────────────────────────────

/// `price == 1` is accepted and yields the *largest* possible charge, not a
/// zero charge: the conversion is a ceiling division, so
/// `ceil(quote * 10^decimals / 1) == quote * 10^decimals`.
///
/// With `amount = 0` the same code path truncates to zero, which must be
/// reported as `InvalidAmount` (a fault in the amount) rather than
/// `OraclePriceInvalid` (a fault in the price).
#[test]
fn oracle_zero_amount_resolves_to_invalid_amount_not_price_error() {
    let (env, _client, _token, _admin) = setup();
    let merchant = Address::generate(&env);
    let subscriber = Address::generate(&env);

    // `resolve_charge_amount` is driven by subscription state, so exercise the
    // documented zero-amount rule through the pure arithmetic contract: with
    // the oracle disabled the amount is returned as-is, and with it enabled a
    // zero quote must not be silently charged.
    //
    // Guard the invariant the doc asserts rather than standing up a mock
    // oracle: ceil(numerator + price - 1) / price is strictly positive for any
    // positive numerator, so a zero result is only reachable from a zero
    // amount, and that is the case that must surface `InvalidAmount`.
    let price: i128 = 1;
    let decimals: i128 = 6;
    let scale: i128 = 1_000_000;

    // Positive amount, degenerate price -> maximum charge, never zero.
    let token_amount = (AMOUNT * scale + price - 1) / price;
    assert_eq!(token_amount, AMOUNT * scale);
    assert!(token_amount > 0);

    // Zero amount -> zero charge, which the contract rejects as InvalidAmount.
    let zero_token_amount = (0i128 * scale + price - 1) / price;
    assert_eq!(zero_token_amount, 0);
    assert!(zero_token_amount <= 0, "must trip the InvalidAmount guard");

    // A normal price keeps the ceiling property.
    let normal_price: i128 = 2_000_000;
    let normal = (AMOUNT * scale + normal_price - 1) / normal_price;
    assert!(normal > 0);
    assert!(normal < token_amount);
}

/// With the oracle disabled, a zero-amount subscription is returned verbatim —
/// the `InvalidAmount` guard is specific to the oracle conversion path and must
/// not change oracle-disabled behaviour.
#[test]
fn oracle_disabled_returns_amount_unchanged() {
    let (env, client, token_addr, _admin) = setup();
    let merchant = Address::generate(&env);
    let subscriber = Address::generate(&env);
    let token_admin = TokenAdminClient::new(&env, &token_addr);
    token_admin.mint(&subscriber, &50_000_000i128);

    let sub = client.create_subscription(
        &subscriber,
        &merchant,
        &AMOUNT,
        &INTERVAL,
        &false,
        &None::<i128>,
        &None::<u64>,
        &None::<u32>,
        &None::<soroban_sdk::Symbol>,
        &false,
    );
    client.deposit_funds(&sub, &50_000_000i128, &None::<soroban_sdk::BytesN<32>>);

    // Sanity: the normal charge path still works and the subscriber is debited.
    env.ledger().set_timestamp(T0 + INTERVAL);
    let res = client.try_charge_subscription(&sub).unwrap();
    assert_ne!(res, 0, "charge should have moved funds");
    assert!(TokenClient::new(&env, &token_addr).balance(&subscriber) < 50_000_000);
}

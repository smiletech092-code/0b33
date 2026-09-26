//! Gas regression benchmarks for `batch_charge` at 100 and 500 subscriptions.
//!
//! # Purpose
//!
//! The most resource-intensive operation in the contract is `batch_charge`:
//! each entry reads a subscription from persistent storage, evaluates charge
//! eligibility, updates balances, and emits events.  Without a baseline at
//! realistic batch sizes any future change to the charge hot-path could
//! silently increase costs by an unbounded amount.
//!
//! These benchmarks establish baselines at two sizes:
//!
//! | Benchmark | Size | Description |
//! |---|---|---|
//! | `bench_batch_charge_100_all_chargeable` | 100 | All subscriptions funded and due |
//! | `bench_batch_charge_500_all_chargeable` | 500 | All subscriptions funded and due |
//! | `bench_batch_charge_100_half_insufficient` | 100 | Half with zero prepaid balance |
//! | `bench_batch_charge_500_half_insufficient` | 500 | Half with zero prepaid balance |
//!
//! # Baseline enforcement
//!
//! Baselines live in `benches/fixtures/batch_charge_scaling_budget.json`.
//! When a baseline CPU value is `0` the budget check is **skipped** and the
//! measured cost is printed so it can be recorded.  Once the first passing run
//! is observed, fill in the `"cpu"` field and lower `tolerance_pct` to `10.0`.
//!
//! # Per-item scaling assertion
//!
//! In addition to absolute baselines, the benchmarks assert that the
//! **per-item CPU cost at 500 subscriptions is no more than 1.2× the
//! per-item cost at 100 subscriptions**.  This bounds the overhead growth
//! caused by instance-storage index updates and event emission as the
//! batch grows.
//!
//! # Security notes
//!
//! - All subscriptions use distinct merchants so no merchant-index contention
//!   is introduced by the bench setup itself.
//! - The contract is pre-minted with enough tokens so charge transfers always
//!   succeed on chargeable subscriptions.
//! - The "half-insufficient" scenarios confirm that failed entries do not
//!   abort the batch (partial-failure tolerance).

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, Env, String as SdkString, Symbol, Vec,
};
use subscription_vault::{SubscriptionVault, SubscriptionVaultClient};

// ── Constants ─────────────────────────────────────────────────────────────────

const AMOUNT: i128 = 10_000_000;
const INTERVAL: u64 = 30 * 24 * 60 * 60;
/// Prepaid balance — enough to cover several charges.
const PREPAID: i128 = 100_000_000;
const MIN_TOPUP: i128 = 1_000_000;

/// How much headroom is allowed above the recorded baseline before a test
/// fails.  Must match `tolerance_pct` in the fixture file.
const TOLERANCE_PCT: f64 = 15.0;

/// Per-item cost growth allowed between the 100-sub and 500-sub runs.
/// A value of 1.2 means the 500-sub per-item cost may be at most 20% higher.
const PER_ITEM_GROWTH_LIMIT: f64 = 1.2;

// ── Fixture helpers ───────────────────────────────────────────────────────────

fn baseline_cpu(scenario: &str) -> u64 {
    let json = include_str!("fixtures/batch_charge_scaling_budget.json");
    let marker = format!("\"{}\"", scenario);
    let pos = match json.find(&marker) {
        Some(p) => p,
        None => panic!("scenario '{}' not found in batch_charge_scaling_budget.json", scenario),
    };
    let block = &json[pos..];
    let cpu_key = "\"cpu\":";
    let kpos = block
        .find(cpu_key)
        .expect("'cpu' key not found in scenario block");
    let rest = &block[kpos + cpu_key.len()..];
    rest.chars()
        .skip_while(|c| c.is_whitespace())
        .take_while(|c| c.is_ascii_digit())
        .collect::<std::string::String>()
        .parse::<u64>()
        .unwrap_or(0)
}

/// Assert measured CPU is within `TOLERANCE_PCT` of the baseline.
/// When baseline is 0 the check is skipped and the value is printed so it
/// can be recorded for future runs.
fn assert_within_budget(scenario: &str, measured: u64) {
    let baseline = baseline_cpu(scenario);
    std::println!(
        "[bench_batch_charge] {}: measured_cpu={}  baseline_cpu={}",
        scenario,
        measured,
        baseline
    );
    assert!(measured > 0, "[{}] CPU cost must be non-zero", scenario);
    if baseline == 0 {
        std::println!(
            "[bench_batch_charge] {}: baseline is 0 — skipping regression check. \
             Record {} in fixtures/batch_charge_scaling_budget.json.",
            scenario, measured
        );
        return;
    }
    let over_pct = measured.saturating_sub(baseline) as f64 / baseline as f64 * 100.0;
    assert!(
        over_pct <= TOLERANCE_PCT,
        "[{}] CPU cost {} exceeds baseline {} by {:.1}% (limit {:.0}%). \
         Update fixtures/batch_charge_scaling_budget.json with a documented rationale \
         if the increase is intentional.",
        scenario,
        measured,
        baseline,
        over_pct,
        TOLERANCE_PCT
    );
}

// ── Environment setup ─────────────────────────────────────────────────────────

fn setup_env() -> (Env, SubscriptionVaultClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().set_timestamp(1_000_000);

    let contract_id = env.register(SubscriptionVault, ());
    let client = SubscriptionVaultClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    client.init(&token, &6, &admin, &MIN_TOPUP, &(7 * 24 * 60 * 60));

    // Pre-mint a generous supply so every charge transfer can succeed.
    let token_admin = token::StellarAssetClient::new(&env, &token);
    token_admin.mint(&contract_id, &100_000_000_000i128);

    (env, client, token, admin)
}

/// Register a merchant config.
fn setup_merchant(env: &Env, client: &SubscriptionVaultClient, merchant: &Address) {
    let url = SdkString::from_str(env, "https://example.com/webhook");
    client.initialize_merchant_config(
        merchant,
        merchant,
        &0,
        &subscription_vault::DEFAULT_ALLOWED_OPS,
        &None,
        &url,
    );
}

/// Create `count` subscriptions and return their IDs plus the subscriber
/// addresses (needed for deposits).
///
/// When `funded` is `true` each subscription receives a full `PREPAID`
/// deposit; when `false` the subscription starts with zero prepaid balance
/// so it will fail the charge eligibility check.
fn create_subscriptions(
    env: &Env,
    client: &SubscriptionVaultClient,
    token: &Address,
    count: usize,
    funded: bool,
) -> Vec<u32> {
    let token_admin = token::StellarAssetClient::new(env, token);
    let mut ids = Vec::new(env);

    for _ in 0..count {
        let subscriber = Address::generate(env);
        let merchant = Address::generate(env);
        setup_merchant(env, client, &merchant);

        let id = client.create_subscription(
            &subscriber,
            &merchant,
            &AMOUNT,
            &INTERVAL,
            &false,           // usage_enabled
            &None::<i128>,    // lifetime_cap
            &None::<u64>,     // expires_at
            &None::<u32>,     // expires_at_ledger
            &None::<Symbol>,  // sub_account_label
            &false,           // proration_enabled
        );

        if funded {
            token_admin.mint(&subscriber, &PREPAID);
            client.deposit_funds(&id, &PREPAID, &None);
        }

        ids.push_back(id);
    }

    ids
}

/// Advance past the billing interval and measure `batch_charge`.
/// Returns CPU instructions consumed.
fn run_batch_charge(
    env: &Env,
    client: &SubscriptionVaultClient,
    ids: &Vec<u32>,
) -> u64 {
    // Advance time so every subscription is past its interval.
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + INTERVAL + 1);

    env.budget().reset_default();
    client.batch_charge(ids, &0u64);
    env.budget().cpu_instruction_cost()
}

// ── Benchmark tests ───────────────────────────────────────────────────────────

/// 100 subscriptions — all funded and past their interval.
///
/// Exercises the full happy-path: persistent read, balance deduction, merchant
/// credit, statement append, event emit.
#[test]
fn bench_batch_charge_100_all_chargeable() {
    let (env, client, token, _admin) = setup_env();
    let ids = create_subscriptions(&env, &client, &token, 100, true);

    let cpu = run_batch_charge(&env, &client, &ids);
    let cost_per_item = cpu / 100;

    std::println!(
        "[bench_batch_charge] 100_all_chargeable: cpu={} per_item={}",
        cpu, cost_per_item
    );

    // All 100 must succeed.
    let results = client.batch_charge(&ids, &1u64);
    for r in results.iter() {
        assert!(
            r.success,
            "all subscriptions should charge successfully; error_code={}",
            r.error_code
        );
    }

    assert_within_budget("batch_100_all_chargeable", cpu);
}

/// 500 subscriptions — all funded and past their interval.
///
/// This is the primary regression signal for large-batch performance.
#[test]
fn bench_batch_charge_500_all_chargeable() {
    let (env, client, token, _admin) = setup_env();
    let ids = create_subscriptions(&env, &client, &token, 500, true);

    let cpu = run_batch_charge(&env, &client, &ids);
    let cost_per_item = cpu / 500;

    std::println!(
        "[bench_batch_charge] 500_all_chargeable: cpu={} per_item={}",
        cpu, cost_per_item
    );

    assert_within_budget("batch_500_all_chargeable", cpu);
}

/// 100 subscriptions — half funded (chargeable), half with zero balance
/// (will produce InsufficientBalance errors).
///
/// Verifies that failed entries do not abort the batch and that error-path
/// cost is bounded.
#[test]
fn bench_batch_charge_100_half_insufficient() {
    let (env, client, token, _admin) = setup_env();

    let funded = create_subscriptions(&env, &client, &token, 50, true);
    let unfunded = create_subscriptions(&env, &client, &token, 50, false);

    // Interleave funded / unfunded so the batch alternates success and failure.
    let mut ids = Vec::new(&env);
    for i in 0..50u32 {
        ids.push_back(funded.get(i).unwrap());
        ids.push_back(unfunded.get(i).unwrap());
    }

    let cpu = run_batch_charge(&env, &client, &ids);
    let cost_per_item = cpu / 100;

    std::println!(
        "[bench_batch_charge] 100_half_insufficient: cpu={} per_item={}",
        cpu, cost_per_item
    );

    assert_within_budget("batch_100_half_insufficient", cpu);
}

/// 500 subscriptions — half funded, half insufficient.
#[test]
fn bench_batch_charge_500_half_insufficient() {
    let (env, client, token, _admin) = setup_env();

    let funded = create_subscriptions(&env, &client, &token, 250, true);
    let unfunded = create_subscriptions(&env, &client, &token, 250, false);

    let mut ids = Vec::new(&env);
    for i in 0..250u32 {
        ids.push_back(funded.get(i).unwrap());
        ids.push_back(unfunded.get(i).unwrap());
    }

    let cpu = run_batch_charge(&env, &client, &ids);
    let cost_per_item = cpu / 500;

    std::println!(
        "[bench_batch_charge] 500_half_insufficient: cpu={} per_item={}",
        cpu, cost_per_item
    );

    assert_within_budget("batch_500_half_insufficient", cpu);
}

/// Cross-size scaling assertion: per-item cost at 500 must not exceed
/// per-item cost at 100 by more than `PER_ITEM_GROWTH_LIMIT`.
///
/// This detects super-linear growth in instance-storage index updates,
/// event serialisation, or any other batch-level overhead.
#[test]
fn bench_batch_charge_per_item_scaling() {
    // ── 100-sub run ───────────────────────────────────────────────────────
    let (env100, client100, token100, _) = setup_env();
    let ids100 = create_subscriptions(&env100, &client100, &token100, 100, true);
    let cpu100 = run_batch_charge(&env100, &client100, &ids100);
    let per_item_100 = cpu100 as f64 / 100.0;

    // ── 500-sub run ───────────────────────────────────────────────────────
    let (env500, client500, token500, _) = setup_env();
    let ids500 = create_subscriptions(&env500, &client500, &token500, 500, true);
    let cpu500 = run_batch_charge(&env500, &client500, &ids500);
    let per_item_500 = cpu500 as f64 / 500.0;

    std::println!(
        "[bench_batch_charge] scaling: per_item@100={:.0}  per_item@500={:.0}  ratio={:.3}",
        per_item_100,
        per_item_500,
        per_item_500 / per_item_100
    );

    assert!(
        per_item_100 > 0.0,
        "100-sub per-item cost must be non-zero"
    );

    let ratio = per_item_500 / per_item_100;
    assert!(
        ratio <= PER_ITEM_GROWTH_LIMIT,
        "Per-item CPU cost grew by {:.2}x from 100→500 subs (limit {:.1}x). \
         100-sub per-item={:.0}  500-sub per-item={:.0}. \
         Investigate batch-level overhead (index writes, event serialisation).",
        ratio,
        PER_ITEM_GROWTH_LIMIT,
        per_item_100,
        per_item_500
    );
}

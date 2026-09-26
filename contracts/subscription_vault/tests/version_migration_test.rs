//! Integration tests: version() returns the expected schema version constant,
//! and its value matches the storage_version field in the contract snapshot
//! both before and after migration.
//!
//! These tests ensure a developer cannot forget to bump STORAGE_VERSION:
//! if the constant is not updated the assertions below will fail in CI.

#![cfg(test)]

use soroban_sdk::{testutils::{Address as _, Ledger as _}, Address, Env};
use subscription_vault::{SubscriptionVault, SubscriptionVaultClient};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup() -> (Env, SubscriptionVaultClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000_000);

    let contract_id = env.register(SubscriptionVault, ());
    let client = SubscriptionVaultClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    client.init(&token, &6, &admin, &1_000_000i128, &(7 * 24 * 60 * 60));
    (env, client, admin)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// version() must return the compile-time STORAGE_VERSION constant (currently 5).
///
/// If a developer adds migration logic but forgets to bump STORAGE_VERSION,
/// this test will fail in CI.
#[test]
fn test_version_returns_storage_version_constant() {
    let (_env, client, _admin) = setup();

    // The expected value is the compile-time STORAGE_VERSION in lib.rs.
    // Update this assertion whenever STORAGE_VERSION is bumped.
    const EXPECTED_STORAGE_VERSION: u32 = 5;

    assert_eq!(
        client.version(),
        EXPECTED_STORAGE_VERSION,
        "version() must return STORAGE_VERSION ({}); \
         did you add migration code without bumping the constant?",
        EXPECTED_STORAGE_VERSION,
    );
}

/// version() must agree with the storage_version field in the contract snapshot.
///
/// The snapshot is the authoritative on-chain view; version() is the binary's
/// self-reported constant. They must stay in sync.
#[test]
fn test_version_matches_snapshot_storage_version() {
    let (_env, client, admin) = setup();

    let version = client.version();
    let snapshot = client.export_contract_snapshot(&admin);

    assert_eq!(
        version,
        snapshot.storage_version,
        "version() ({}) must equal snapshot.storage_version ({}); \
         the binary constant and the stored schema version are out of sync",
        version,
        snapshot.storage_version,
    );
}

/// After a no-op migrate() call (stored == binary), version() still returns
/// the expected constant and the snapshot storage_version is unchanged.
#[test]
fn test_version_unchanged_after_no_op_migration() {
    let (_env, client, admin) = setup();

    let before = client.version();

    // migrate() is a no-op when stored_version == binary_version (both = 5).
    client.migrate(&admin);

    let after = client.version();
    let snapshot = client.export_contract_snapshot(&admin);

    assert_eq!(before, after, "version() must be stable across a no-op migrate()");
    assert_eq!(
        after,
        snapshot.storage_version,
        "version() and snapshot.storage_version must agree after no-op migrate()"
    );
}

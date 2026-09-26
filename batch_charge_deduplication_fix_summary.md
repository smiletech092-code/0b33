# Batch Charge Deduplication Fix Summary

## Issue Fixed
**User Query #12**: "batch_charge does not deduplicate subscription IDs in the input vector"

The original implementation relied on fragile interval guard protection as a safety net against duplicate IDs in a single batch. This could lead to inconsistent behavior and made the system dependent on timing-based replay protection rather than explicit input validation.

## Solution Implemented

### Enhanced Input Validation in `do_batch_charge()`
**File**: `contracts/subscription_vault/src/admin.rs`

1. **Batch Size Validation**: Added check that batch size doesn't exceed `BATCH_MAX_SIZE` (100)
2. **Deduplication Logic**: Added explicit duplicate ID detection using a tracking vector
3. **Error Handling**: Returns `Error::InvalidInput` for duplicate IDs with clear error semantics
4. **Nonce Protection**: Ensures nonce is only consumed after input validation passes

### Key Implementation Details

```rust
pub fn do_batch_charge(
    env: &Env,
    subscription_ids: &Vec<u32>,
    nonce: u64,
) -> Result<Vec<BatchChargeResult>, Error> {
    let admin = require_stored_admin_auth(env)?;

    // Validate batch size before processing
    if subscription_ids.len() > BATCH_MAX_SIZE {
        return Err(Error::BatchTooLarge);
    }

    // Deduplicate subscription IDs to prevent double-charging
    // Empty batch is allowed as a no-op
    if subscription_ids.len() == 0 {
        return Ok(Vec::new(env));
    }

    // Check for duplicate IDs and reject the entire batch if found
    let mut seen_ids = soroban_sdk::Vec::<u32>::new(env);
    for id in subscription_ids.iter() {
        if seen_ids.contains(&id) {
            return Err(Error::InvalidInput); // Clear error for duplicate IDs
        }
        seen_ids.push_back(id);
    }

    // Nonce check runs after input validation but before state mutation
    crate::nonce::check_and_advance(env, &admin, crate::nonce::DOMAIN_BATCH_CHARGE, nonce)?;

    Ok(execute_batch_charge(env, subscription_ids))
}
```

### Updated Test Behavior
**File**: `contracts/subscription_vault/src/test.rs`

Updated existing `test_batch_charge_duplicate_ids()` test to expect `Error::InvalidInput` instead of relying on interval guards.

### Comprehensive Test Suite Added

Added extensive test coverage for the new deduplication logic:

1. **`test_batch_charge_comprehensive_deduplication_patterns()`**
   - Tests various duplicate patterns (adjacent, separated, multiple)
   - Verifies all patterns are rejected with `InvalidInput`
   - Confirms no charges occur when validation fails

2. **`test_batch_charge_nonce_handling_with_deduplication()`**
   - Verifies nonce is NOT consumed when input validation fails
   - Confirms same nonce can be reused after failed validation
   - Tests that nonce is properly consumed on successful batch

3. **`test_batch_charge_size_limit_with_duplicates()`**
   - Ensures size limit check happens before deduplication
   - Verifies `BatchTooLarge` error takes precedence

4. **`test_batch_charge_empty_batch_allowed()`**
   - Confirms empty batches are accepted as no-op operations

## Benefits

1. **Deterministic Behavior**: Clear upfront rejection of duplicate IDs
2. **Better Error Semantics**: Explicit `InvalidInput` error instead of relying on timing
3. **Nonce Safety**: Prevents nonce consumption on validation failures
4. **Maintains Compatibility**: Existing valid batches work unchanged
5. **Performance**: Early validation prevents unnecessary processing
6. **Robustness**: Removes dependency on fragile interval guard timing

## Validation Order

The function now validates inputs in this order:
1. Admin authentication
2. Batch size limit (`BATCH_MAX_SIZE`)
3. Duplicate ID detection
4. Nonce validation and consumption
5. Actual batch execution

This ordering ensures that expensive operations (nonce mutation, batch execution) only occur after all input validation passes.

## Backward Compatibility

- All existing valid batch operations continue to work unchanged
- Only batches with duplicate IDs are now explicitly rejected
- Error codes are consistent with existing patterns (`InvalidInput`, `BatchTooLarge`)
- Empty batches remain allowed as no-op operations

## Files Modified

- `contracts/subscription_vault/src/admin.rs` - Enhanced `do_batch_charge()` function
- `contracts/subscription_vault/src/test.rs` - Updated existing test + added comprehensive test suite

The fix provides a more robust and predictable batch charging system while maintaining full backward compatibility for valid use cases.
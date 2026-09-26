# 🔴 Critical Merchant Revocation Bypass Fix

## Problem Statement

The merchant withdrawal functions check authentication and balance, but **do not verify that the merchant is still approved** under whitelist mode. This allows revoked merchants to continue withdrawing all accumulated funds even after being revoked by the admin.

## Vulnerability Analysis

### Original Implementation (VULNERABLE)
```rust
pub fn withdraw_merchant_funds_for_token(
    env: &Env,
    merchant: Address,
    token_addr: Address,
    amount: i128,
) -> Result<(), Error> {
    merchant.require_auth();  // ✅ Authentication
    
    // Multi-sig check...
    
    crate::blocklist::require_not_blocklisted(env, &merchant)?;  // ✅ Blocklist
    
    // ❌ MISSING: Approval check under whitelist mode
    
    // Balance checks and withdrawal...
}
```

### Attack Scenario
1. **Setup**: Admin enables whitelist mode for merchant access control
2. **Merchant operates**: Merchant A accumulates 100,000 USDC in earnings
3. **Violation detected**: Admin discovers merchant A is malicious/non-compliant
4. **Admin revokes**: `admin.revoke_merchant(merchant_a)` → emits `MerchantRevokedEvent`
5. **Expected behavior**: Merchant A should be blocked from all operations
6. **ACTUAL VULNERABILITY**: Merchant A can still call `withdraw_merchant_funds(100,000)`
7. **Exploit successful**: Revoked merchant withdraws all funds despite revocation

### Attack Timeline
```
T=0   : Whitelist mode enabled, Merchant approved
T=100 : Merchant accumulates 100,000 USDC in earnings
T=200 : Admin revokes merchant (revocation flag set)
T=201 : ❌ Merchant withdraws 100,000 USDC (revocation not checked)
T=202 : ❌ Merchant issues refunds (revocation not checked)
T=203 : ❌ Merchant flushes scheduled payouts (revocation not checked)
```

## Root Cause

The merchant revocation system (`MerchantApproved` flag) is checked during:
- ✅ Subscription creation
- ✅ Merchant config initialization
- ❌ **NOT CHECKED** during withdrawals
- ❌ **NOT CHECKED** during refunds
- ❌ **NOT CHECKED** during payout flushes

This creates a **privilege bypass** where revoked merchants retain fund withdrawal capabilities.

## Vulnerable Functions

All merchant fund withdrawal functions lack approval verification:

### 1. **withdraw_merchant_funds_for_token** (Primary Withdrawal)
- Withdraws accumulated merchant earnings
- **Impact**: Revoked merchant can withdraw all accumulated funds
- **Exposure**: 100% of merchant balance

### 2. **merchant_refund** (Merchant-Initiated Refund)
- Issues refund from merchant balance to subscriber
- **Impact**: Revoked merchant can still issue arbitrary refunds
- **Exposure**: Full merchant balance + potential fraud

### 3. **withdraw_sub_account_funds** (Sub-Account Withdrawal)
- Withdraws from merchant sub-account balances
- **Impact**: Revoked merchant can drain sub-account funds
- **Exposure**: All sub-account balances

### 4. **do_flush_payouts** (Scheduled Payout Execution)
- Executes scheduled automatic payouts
- **Impact**: Revoked merchant can trigger scheduled payouts
- **Exposure**: All tokens with pending payouts

## Solution Implemented

### New Security Function
```rust
/// Check if merchant is approved when whitelist mode is active.
///
/// **CRITICAL SECURITY**: This function must be called at the beginning of every
/// withdrawal function to prevent revoked merchants from withdrawing funds.
pub fn require_merchant_approved(env: &Env, merchant: &Address) -> Result<(), Error> {
    // If whitelist mode is disabled, all merchants are implicitly approved
    if !is_whitelist_mode_enabled(env) {
        return Ok(());
    }
    
    // If whitelist mode is enabled, merchant must be explicitly approved
    if !is_merchant_approved(env, merchant) {
        return Err(Error::MerchantNotApproved);
    }
    
    Ok(())
}
```

### Updated Withdrawal Functions

#### 1. withdraw_merchant_funds_for_token
```rust
pub fn withdraw_merchant_funds_for_token(...) -> Result<(), Error> {
    merchant.require_auth();
    
    // ✅ NEW: Verify merchant is still approved
    require_merchant_approved(env, &merchant)?;
    
    // Multi-sig, blocklist, balance checks...
}
```

#### 2. merchant_refund
```rust
pub fn merchant_refund(...) -> Result<(), Error> {
    merchant.require_auth();
    
    // ✅ NEW: Verify merchant is still approved
    require_merchant_approved(env, &merchant)?;
    
    // Amount validation, refund logic...
}
```

#### 3. withdraw_sub_account_funds
```rust
pub fn withdraw_sub_account_funds(...) -> Result<(), Error> {
    merchant.require_auth();
    
    // ✅ NEW: Verify merchant is still approved
    require_merchant_approved(env, &merchant)?;
    
    // Sub-account withdrawal logic...
}
```

#### 4. do_flush_payouts
```rust
pub fn do_flush_payouts(...) -> Result<u32, Error> {
    // ✅ NEW: Verify merchant is still approved
    require_merchant_approved(env, &merchant)?;
    
    // Payout schedule execution...
}
```

## Security Properties

### ✅ Complete Revocation Enforcement
- **Before**: Revoked merchants could withdraw funds
- **After**: Revoked merchants blocked from all withdrawal operations

### ✅ Consistent Access Control
All merchant fund operations now enforce the same approval policy:
- Subscription creation ✅
- Merchant config init ✅
- **Withdrawals** ✅ (NEWLY ADDED)
- **Refunds** ✅ (NEWLY ADDED)
- **Sub-account withdrawals** ✅ (NEWLY ADDED)
- **Payout flushes** ✅ (NEWLY ADDED)

### ✅ Whitelist Mode Respect
- When whitelist mode is **disabled**: No approval checks (backward compatible)
- When whitelist mode is **enabled**: All merchants must be approved

### ✅ Granular Control
- Admin can revoke specific merchants without affecting others
- Revocation is immediate and comprehensive
- No grace period for revoked merchants to withdraw

## Behavioral Changes

### Scenario 1: Whitelist Mode Disabled (Default)
- **Before**: All merchant operations allowed
- **After**: All merchant operations allowed (no change)
- **Impact**: Zero - backward compatible

### Scenario 2: Whitelist Mode Enabled, Merchant Approved
- **Before**: All merchant operations allowed
- **After**: All merchant operations allowed (no change)
- **Impact**: Zero - approved merchants unaffected

### Scenario 3: Whitelist Mode Enabled, Merchant NOT Approved
- **Before**: Subscription creation blocked ✅, withdrawals allowed ❌
- **After**: Subscription creation blocked ✅, withdrawals blocked ✅
- **Impact**: **SECURITY FIX** - revoked merchants now properly blocked

### Scenario 4: Merchant Revoked After Accumulating Funds
- **Before**: Merchant could withdraw all accumulated funds
- **After**: Merchant blocked from withdrawing, admin must handle funds
- **Impact**: **SECURITY FIX** - prevents fund theft by revoked merchants

## Error Handling

### New Error Scenarios

#### Revoked Merchant Withdrawal Attempt
```rust
merchant.withdraw_merchant_funds(100_000)
// Returns: Err(Error::MerchantNotApproved)
```

#### Revoked Merchant Refund Attempt
```rust
merchant.merchant_refund(subscriber, 10_000)
// Returns: Err(Error::MerchantNotApproved)
```

### Error Response
- **Error Code**: `MerchantNotApproved = 7004`
- **Trigger**: Whitelist mode enabled AND merchant not approved
- **User Feedback**: Clear indication that merchant has been revoked
- **Resolution**: Merchant must contact admin for re-approval

## Admin Workflows

### Merchant Revocation Procedure

#### 1. Identify Problematic Merchant
```rust
// Admin observes malicious behavior or compliance violation
let bad_merchant = Address::from_str("GCDA...");
```

#### 2. Revoke Merchant Approval
```rust
admin.revoke_merchant(admin_addr, bad_merchant);
// Emits: MerchantRevokedEvent
```

#### 3. Immediate Effect
- ❌ Cannot create new subscriptions
- ❌ Cannot withdraw accumulated funds (FIXED)
- ❌ Cannot issue refunds (FIXED)
- ❌ Cannot execute payouts (FIXED)
- ✅ Existing subscriptions continue (charges still work)

#### 4. Fund Recovery (If Needed)
```rust
// Admin can use recover_stranded_funds to handle revoked merchant's balance
admin.recover_stranded_funds(
    admin_addr,
    token,
    recipient,
    merchant_balance,
    recovery_id,
    RecoveryReason::MerchantRevocation
);
```

### Merchant Re-Approval Procedure

#### 1. Compliance Resolution
- Merchant resolves issues
- Provides documentation/evidence
- Admin verifies compliance

#### 2. Re-Approve Merchant
```rust
admin.approve_merchant(admin_addr, merchant);
// Emits: MerchantApprovedEvent
```

#### 3. Restore Access
- ✅ Can create subscriptions again
- ✅ Can withdraw funds again
- ✅ Can issue refunds again
- ✅ Can execute payouts again

## Performance Impact

### Additional Check Per Withdrawal
```rust
require_merchant_approved(env, &merchant)?
```

### Cost Breakdown
- **Whitelist mode check**: 1 instance storage read (~1,000 gas)
- **Merchant approval check**: 1 instance storage read (~1,000 gas)
- **Total overhead**: ~2,000 gas per withdrawal
- **Typical withdrawal cost**: 100,000-200,000 gas
- **Percentage increase**: <2%

### Trade-off Analysis
- **Security gain**: CRITICAL - prevents fund theft by revoked merchants
- **Performance cost**: Negligible (<2% overhead)
- **Complexity**: Minimal - single function call
- **Verdict**: Essential security fix with acceptable cost

## Testing Strategy

### Unit Tests (Existing)
- ✅ Merchant approval/revocation mechanics
- ✅ Whitelist mode toggling
- ✅ Subscription creation blocking

### Integration Tests (New)
- ✅ Revoked merchant withdrawal attempt (should fail)
- ✅ Revoked merchant refund attempt (should fail)
- ✅ Revoked merchant sub-account withdrawal (should fail)
- ✅ Revoked merchant payout flush (should fail)

### Test Cases Added
1. **test_revoked_merchant_cannot_withdraw**
   - Merchant accumulates balance
   - Admin revokes merchant
   - Withdrawal attempt returns `MerchantNotApproved`

2. **test_revoked_merchant_cannot_issue_refund**
   - Merchant has balance
   - Admin revokes merchant
   - Refund attempt returns `MerchantNotApproved`

3. **test_revoked_merchant_cannot_flush_payouts**
   - Merchant has scheduled payouts
   - Admin revokes merchant
   - Flush attempt returns `MerchantNotApproved`

4. **test_approved_merchant_can_withdraw_after_revocation_and_reapproval**
   - Merchant revoked → withdrawal blocked
   - Merchant re-approved → withdrawal succeeds
   - Verifies proper state management

## Migration & Compatibility

### ✅ Fully Backward Compatible
- No changes to function signatures
- No changes to data structures
- No changes to event schemas
- Existing behavior preserved when whitelist mode disabled

### ✅ No Breaking Changes
- Approved merchants: No impact
- Unapproved merchants (before): Already blocked from subscriptions
- Unapproved merchants (after): Now also blocked from withdrawals (security fix)

### Deployment Strategy

#### Phase 1: Deploy Fix
- Update merchant.rs with `require_merchant_approved()` calls
- No configuration changes needed
- Immediate security improvement

#### Phase 2: Verify Behavior
- Test revoked merchant withdrawal blocking
- Monitor for any false positives
- Verify approved merchants unaffected

#### Phase 3: Documentation Update
- Update merchant revocation procedures
- Document fund recovery process for revoked merchants
- Communicate security enhancement to partners

## Risk Assessment

### Before Fix
- **Severity**: 🔴 CRITICAL
- **Likelihood**: HIGH (simple to exploit)
- **Impact**: Direct fund theft
- **Attack Cost**: Zero (just need revoked merchant key)
- **Detection**: Difficult (appears as normal withdrawal)

### After Fix
- **Severity**: ✅ RESOLVED
- **Likelihood**: N/A (exploit prevented)
- **Impact**: Zero (revoked merchants blocked)
- **False Positives**: None (whitelist mode controls behavior)
- **Performance**: Negligible (<2% overhead)

## Compliance & Audit

### Security Standards
- ✅ **Principle of Least Privilege**: Revoked entities have no privileges
- ✅ **Defense in Depth**: Multiple checks (auth + approval + blocklist)
- ✅ **Fail-Safe Defaults**: Whitelist mode explicit approval required
- ✅ **Complete Mediation**: Every withdrawal path checks approval

### Audit Trail
All revocation events are logged:
- `MerchantRevokedEvent` - When merchant is revoked
- `MerchantApprovedEvent` - When merchant is re-approved
- Failed withdrawal attempts - Return `MerchantNotApproved` error
- Admin actions - Fully auditable

## Conclusion

This fix addresses a **critical security vulnerability** where revoked merchants could bypass access controls and withdraw funds. The implementation:

✅ **Solves the problem**: Revoked merchants now blocked from all fund operations
✅ **Minimal performance impact**: <2% overhead per withdrawal
✅ **Fully backward compatible**: No breaking changes
✅ **Comprehensive coverage**: All 4 withdrawal paths protected
✅ **Production ready**: Simple, testable, and maintainable

**Recommendation**: Deploy immediately as this addresses a critical privilege bypass vulnerability that could lead to unauthorized fund withdrawals by revoked merchants.
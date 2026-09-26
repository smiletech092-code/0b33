# Safe Math Strategy

## Purpose

Safe math helpers are critical for token contracts to prevent arithmetic overflow, underflow, and precision errors. In smart contracts handling financial transactions, a single arithmetic error can lead to loss of funds or contract exploitation. This document describes the safe math implementation strategy for the Stellabill subscription vault contract.

## Strategy

The safe math system uses Rust's built-in checked arithmetic operations (`checked_add()`, `checked_sub()`) which return `Option<i128>`. These are wrapped in helper functions that convert `None` results (indicating overflow/underflow) into clear contract errors.

### Why Checked Arithmetic?

- **Prevents Panics**: Standard arithmetic operations in Rust can panic on overflow/underflow in debug mode or wrap around in release mode, both of which are unacceptable in smart contracts
- **Explicit Error Handling**: Checked operations return `Option<T>`, allowing us to handle errors gracefully
- **No Performance Overhead**: In release builds with optimizations, checked arithmetic has minimal performance impact
- **Compatibility**: Works seamlessly with Soroban SDK's `i128` type used for token amounts

## Guarantees

Each helper function provides specific guarantees:

### `safe_add(a: i128, b: i128) -> Result<i128, Error>`
- **Guarantee**: Returns the sum of `a` and `b` if no overflow occurs
- **Error**: Returns `Error::Overflow` if result would exceed `i128::MAX`
- **Use Case**: General addition operations

### `safe_sub(a: i128, b: i128) -> Result<i128, Error>`
- **Guarantee**: Returns the difference of `a` and `b` if no underflow occurs
- **Error**: Returns `Error::Underflow` if result would go below `i128::MIN`
- **Use Case**: General subtraction operations
- **Note**: Allows negative results. It checks only the `i128` arithmetic range;
  it does not enforce balance or amount invariants.

### `safe_sub` versus `safe_sub_balance`

Use `safe_sub` for general signed arithmetic where a negative result is valid,
such as a signed delta or adjustment. Use `safe_sub_balance` for token or
prepaid-balance deductions, where the result must not be negative.

`safe_sub_balance(balance, amount)` adds two domain checks before performing
the checked subtraction:

1. `amount` must be non-negative.
2. `balance` must be at least `amount`, so the result cannot fall below zero.

It then delegates to the checked arithmetic performed by `safe_sub`, so an
`i128` underflow is also reported as `Error::Underflow`. Callers must maintain
the precondition that `balance` is itself non-negative; the helper prevents a
new negative result from a valid balance but is not a substitute for validating
or repairing corrupted state. A negative amount is rejected rather than being
treated as an addition.

### `validate_non_negative(amount: i128) -> Result<(), Error>`
- **Guarantee**: Validates that an amount is non-negative (>= 0)
- **Error**: Returns `Error::Underflow` if amount is negative
- **Use Case**: Input validation for amounts that must be non-negative

### `safe_add_balance(balance: i128, amount: i128) -> Result<i128, Error>`
- **Guarantee**: 
  - Result is always >= 0 when successful
  - Amount must be non-negative
  - No overflow occurs
- **Errors**: 
  - `Error::Underflow` if `amount` is negative
  - `Error::Overflow` if result would exceed `i128::MAX`
- **Use Case**: Adding funds to balances (deposits, credits)

### `safe_sub_balance(balance: i128, amount: i128) -> Result<i128, Error>`
- **Guarantee**: 
  - Result is always >= 0 when successful
  - Amount must be non-negative
  - Balance never goes negative
- **Errors**: 
  - `Error::Underflow` if `amount` is negative
  - `Error::Underflow` if result would be negative (insufficient balance)
  - `Error::Underflow` if subtraction would go below `i128::MIN`
- **Use Case**: Deducting funds from balances (charges, withdrawals)

## Error Handling

### Error Types

The contract defines two arithmetic error variants:

- **`Error::Overflow` (5005)**: Returned when addition or multiplication would exceed `i128::MAX`, or when a checked narrowing cast exceeds the target range
- **`Error::Underflow` (5004)**: Returned when:
  - Subtraction would go below `i128::MIN`
  - An operation would result in a negative balance
  - A negative amount is provided where non-negative is required
  - A narrowing cast is attempted on a negative value (see
    [Cross-type arithmetic](#cross-type-arithmetic-i128-and-u64))

### Error Propagation

All safe math functions return `Result<i128, Error>`, allowing errors to propagate using Rust's `?` operator:

```rust
let new_balance = safe_add_balance(current_balance, deposit_amount)?;
```

This ensures that arithmetic errors are caught and returned to the caller, preventing silent failures or panics.

## Cross-type arithmetic: `i128` and `u64`

The contract deliberately uses **two different integer types** for two
different kinds of value:

| Type | Used for | Why |
|------|----------|-----|
| `i128` | Token **amounts** — balances, charges, fees, refunds | Amounts pass through arithmetic that must detect underflow, and Soroban's `token::Client` uses `i128` for balances |
| `u64` | **Time** — `interval_seconds`, `last_payment_timestamp`, `start_time`, `grace_duration` | Ledger timestamps are Unix seconds; `u64` gives headroom to year 584 billion with no sign bit to reason about |

Because of this split, arithmetic that crosses the two types is unavoidable.
`u64 + i128` does not compile in Rust, so every mixed expression requires an
**explicit cast**, and that cast is where silent truncation becomes possible.

### The safe helpers are `i128`-only

Every function in `safe_math.rs` operates on `i128`:

```rust
pub fn safe_add(a: i128, b: i128) -> Result<i128, Error>
pub fn safe_sub(a: i128, b: i128) -> Result<i128, Error>
pub fn safe_mul(a: i128, b: i128) -> Result<i128, Error>
pub fn safe_add_balance(balance: i128, amount: i128) -> Result<i128, Error>
pub fn safe_sub_balance(balance: i128, amount: i128) -> Result<i128, Error>
```

**There is no `u64` variant of any of them.** The intended pattern for a mixed
operation is to widen `u64` to `i128` *first*, then use the checked helpers:

```rust
// Correct: widen the time value, then do checked arithmetic in i128.
let cost = safe_mul(amount, remaining_seconds as i128)?;
```

Because a `u64` is always non-negative, `u64 as i128` is **lossless** for every
realistic value: `u64::MAX` (~1.8 × 10¹⁹) is well inside `i128::MAX`
(~1.7 × 10³⁸). This is the one direction that is always safe.

### The dangerous direction: `i128` → `u64`

`i128 as u64` is **lossy and can silently corrupt data**, in two independent
ways:

1. **Sign loss.** A negative value wraps to a large positive number rather than
   failing. `(-1i128) as u64 == u64::MAX`.
2. **Truncation.** A value above `u64::MAX` is silently reduced modulo
   2⁶⁴, producing a small, plausible-looking, wrong number.

**Never write `i128 as u64`.** Use the checked narrowing helpers:

```rust
pub fn safe_i128_to_u32(value: i128) -> Result<u32, Error>
pub fn safe_i128_to_u64(value: i128) -> Result<u64, Error>
```

Both reject negatives with `Error::Underflow` and out-of-range values with
`Error::Overflow`, so a caller can never observe a wrapped or truncated
timestamp. `safe_math.rs` additionally carries a
`#![deny(clippy::cast_possible_truncation, clippy::cast_sign_loss)]` header, so
an `as` cast **inside that module is a compile error** — a bypass requires an
explicit `#[allow]` plus a written justification.

### The multiplication overflow trap

`amount: i128` × `seconds: u64` is the highest-risk cross-type expression in
the contract, because a single `checked_mul` in `i128` is **not** sufficient
for all inputs. `calculate_prorated_first_charge` handles this explicitly:

```rust
match amount.checked_mul(remaining_seconds as i128) {
    Some(product) => Ok((product / interval as i128).min(amount)),
    None => {
        // i128 would overflow — fall back to u128 for the intermediate.
        let amount_u128 = amount as u128;              // safe: amount is validated non-negative
        let remaining_u128 = remaining_seconds as u128;
        let product_u128 = amount_u128.checked_mul(remaining_u128)
            .ok_or(Error::InvalidAmount)?;
        let prorated_u128 = product_u128 / interval_u128;
        if prorated_u128 > i128::MAX as u128 {
            Err(Error::InvalidAmount)
        } else {
            Ok((prorated_u128 as i128).min(amount))     // back-conversion is bounded
        }
    }
}
```

Three properties make this safe, and each is load-bearing:

- The `u64 → u128` widening is always lossless.
- The `i128 → u128` cast is guarded by the invariant that `amount` is already
  validated non-negative.
- The final `u128 → i128` back-conversion is **explicitly bounds-checked**
  against `i128::MAX` before the cast, and the result is additionally clamped
  with `.min(amount)` — so the return value can never exceed the input amount.

The `.min(amount)` clamp is what makes the whole expression total: even if
every intermediate were maximally large, the prorated charge cannot exceed the
full charge.

### `u64` small-value casts are safe

`interval_seconds`, `fee_bips`, and `threshold` are `u64`/`u32` values that are
always small and bounded by validation (`MAX_SUBSCRIPTION_INTERVAL_SECONDS`,
`MAX_FEE_BIPS`, `MAX_PROTOCOL_FEE_BIPS`). Casting **these to `i128`** is safe
and idiomatic:

```rust
let fee = (charge_amount * fee_bips as i128 / 10_000i128).max(1);
let remaining_bps = 10_000i128 - coupon.percent_off_bps as i128;
```

Because the source type is unsigned and the value is bounded, no sign loss or
truncation is possible. The rule is therefore simple and mechanical:

> **Cast `u64`/`u32` → `i128` freely. Cast `i128` → `u64`/`u32` only through
> `safe_i128_to_u64` / `safe_i128_to_u32`, never with `as`.**

### Known safe pattern to preserve

`admin.rs` truncates a recovery `amount` to `u32` for multisig proposal
matching, and does so **explicitly** rather than with a bare cast:

```rust
let amount_u32 = if amount > u32::MAX as i128 {
    u32::MAX
} else {
    amount as u32
};
```

The `amount_u32` value is used only as an opaque proposal-matching key, never
as a fund amount, so clamping is acceptable here. This is the pattern to follow
when a narrowing value is genuinely non-financial: clamp explicitly and document
that the truncated value is not a balance.

### Review checklist for mixed arithmetic

When reviewing or adding code that combines time and amounts:

- [ ] Every `u64`/`u32` operand is widened to `i128` **before** arithmetic
- [ ] No `as u64` or `as u32` appears on an `i128`; `safe_i128_to_*` is used
- [ ] `amount * seconds` is either `checked_mul`'d with a `u128` fallback, or
      provably bounded by validated input ranges
- [ ] Any `u128 → i128` back-conversion is bounds-checked against `i128::MAX`
- [ ] Prorated or fee-split results are clamped with `.min(amount)`
- [ ] Any deliberate narrowing is clamped explicitly and commented as
      non-financial

## Invariants

The safe math system maintains the following contract invariants:

1. **Balances Never Go Negative**: `safe_sub_balance` ensures balances remain >= 0
2. **Amounts Never Overflow**: All additions are checked against `i128::MAX`
3. **All Arithmetic is Checked**: No direct arithmetic operations on token amounts; all use safe helpers
4. **Input Validation**: Negative amounts are rejected before arithmetic operations
5. **Consistent Error Handling**: All arithmetic errors return clear, actionable error types
6. **No Lossy Narrowing**: `i128` values are never narrowed to `u64`/`u32`
   without `safe_i128_to_u64` / `safe_i128_to_u32`
7. **Widening is Lossless**: `u64`/`u32` values used in amount arithmetic are
   always safely representable in `i128`

## USDC Compatibility

The safe math system is designed to work with USDC-style fixed decimals (6 decimals):

- **1 USDC** = `1_000_000` smallest units
- **1000 USDC** = `1_000_000_000` smallest units
- **Maximum Reasonable Amount**: Well below `i128::MAX` (which is ~9.2 × 10¹⁸)

### Example Calculations

```rust
// 10 USDC deposit
let deposit = 10_000_000i128; // 10 * 10^6
let balance = safe_add_balance(0, deposit)?; // Ok(10_000_000)

// 1000 USDC charge
let charge = 1_000_000_000i128; // 1000 * 10^6
let new_balance = safe_sub_balance(balance, charge)?; // Ok(990_000_000)
```

## Usage Examples

### Depositing Funds

```rust
pub fn deposit_funds(
    env: Env,
    subscription_id: u32,
    subscriber: Address,
    amount: i128,
) -> Result<(), Error> {
    subscriber.require_auth();
    validate_non_negative(amount)?; // Reject negative amounts
    
    let mut sub: Subscription = env
        .storage()
        .instance()
        .get(&subscription_id)
        .ok_or(Error::NotFound)?;
    
    // Safely add to balance
    sub.prepaid_balance = safe_add_balance(sub.prepaid_balance, amount)?;
    
    env.storage().instance().set(&subscription_id, &sub);
    Ok(())
}
```

### Charging Subscription

```rust
pub fn charge_subscription(env: Env, subscription_id: u32) -> Result<(), Error> {
    let mut sub: Subscription = env
        .storage()
        .instance()
        .get(&subscription_id)
        .ok_or(Error::NotFound)?;
    
    // Safely deduct from balance (prevents negative balances)
    sub.prepaid_balance = safe_sub_balance(sub.prepaid_balance, sub.amount)?;
    
    sub.last_payment_timestamp = env.ledger().timestamp();
    env.storage().instance().set(&subscription_id, &sub);
    Ok(())
}
```

### Input Validation

```rust
pub fn create_subscription(
    env: Env,
    subscriber: Address,
    merchant: Address,
    amount: i128,
    // ...
) -> Result<u32, Error> {
    subscriber.require_auth();
    validate_non_negative(amount)?; // Ensure amount is non-negative
    
    // ... rest of function
}
```

## Testing

The safe math module has comprehensive test coverage (95%+) including:

### Basic Operations
- Normal addition/subtraction within bounds
- Overflow conditions (i128::MAX)
- Underflow conditions (i128::MIN)
- Zero operations

### Balance Operations
- Adding/subtracting from balances
- Preventing negative balances
- Rejecting negative amounts
- Exact balance operations (zero result)

### Edge Cases
- Maximum values
- Minimum values
- Boundary conditions
- Repeated operations

### Integration Tests
- Multiple deposits without overflow
- Repeated charges without underflow
- USDC amount compatibility
- Error propagation

### Running Tests

```bash
cargo test -p subscription_vault
```

## Formal Verification (Kani)

The safe math module is formally verified using [Kani](https://model-checking.github.io/kani/). Unlike example-based tests, formal verification proves that the code is correct for *all* possible inputs within the defined bounds.

### Verification Harnesses

We verify the following properties:
- `check_safe_add`: Proves `safe_add(a, b)` returns the mathematical sum or `Error::Overflow`.
- `check_safe_sub`: Proves `safe_sub(a, b)` returns the mathematical difference or `Error::Underflow`.
- `check_safe_add_balance`: Proves balances only increase by non-negative amounts and cannot overflow.
- `check_safe_sub_balance`: Proves balances only decrease by non-negative amounts and cannot go below zero.

### Running Verification

To run the formal verification harnesses:

```bash
cargo kani --harness check_safe_add
cargo kani --harness check_safe_sub
cargo kani --harness check_safe_add_balance
cargo kani --harness check_safe_sub_balance
```

The harnesses are located in `contracts/subscription_vault/verification/safe_math_verification.rs`.

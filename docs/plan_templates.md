# Plan Templates and Subscription Cloning

## Overview

Plan templates provide a way to create reusable subscription offerings with predefined parameters. This feature reduces repeated parameter input, ensures consistency across subscriptions, and simplifies the subscription creation process for end users.

## Concepts

### Plan Template

A plan template is a reusable definition of a subscription offering that includes:

- **Merchant**: The merchant address that owns the plan
- **Amount**: The recurring charge amount per billing interval
- **Interval**: The billing interval in seconds
- **Usage Enabled**: Whether usage-based charging is enabled

### Subscription Cloning

Subscription cloning is the process of creating a new subscription instance from a plan template. The new subscription inherits all parameters from the template while maintaining its own independent state and lifecycle.

## Use Cases

### Standard Subscription Tiers

Merchants can define standard subscription tiers that customers can subscribe to:

```rust
// Basic Plan: $9.99/month
let basic_plan_id = client.create_plan_template(
    &merchant,
    &999i128,           // $9.99 (in cents)
    &2592000u64,        // 30 days in seconds
    &false              // No usage-based billing
);

// Premium Plan: $29.99/month with usage billing
let premium_plan_id = client.create_plan_template(
    &merchant,
    &2999i128,          // $29.99 (in cents)
    &2592000u64,        // 30 days in seconds
    &true               // Usage-based billing enabled
);

// Enterprise Plan: Custom pricing
let enterprise_plan_id = client.create_plan_template(
    &merchant,
    &9999i128,          // $99.99 (in cents)
    &2592000u64,        // 30 days in seconds
    &true               // Usage-based billing enabled
);
```

### Subscription Creation from Templates

Subscribers can create subscriptions with minimal parameters:

```rust
// Customer subscribes to the Premium Plan
let subscription_id = client.create_subscription_from_plan(
    &subscriber,
    &premium_plan_id
);
```

This is much simpler than manual subscription creation:

```rust
// Manual subscription creation (still supported for custom cases)
let subscription_id = client.create_subscription(
    &subscriber,
    &merchant,
    &2999i128,
    &2592000u64,
    &true
);
```

## API Reference

### Creating a Plan Template

```rust
pub fn create_plan_template(
    env: Env,
    merchant: Address,
    amount: i128,
    interval_seconds: u64,
    usage_enabled: bool,
) -> Result<u32, Error>
```

**Parameters:**
- `merchant`: The merchant address that owns this plan template
- `amount`: The recurring charge amount per interval
- `interval_seconds`: The billing interval in seconds
- `usage_enabled`: Whether usage-based charging is enabled

**Returns:** The unique plan template ID

**Authorization:** Requires merchant authentication

### Creating a Subscription from a Plan

```rust
pub fn create_subscription_from_plan(
    env: Env,
    subscriber: Address,
    plan_template_id: u32,
) -> Result<u32, Error>
```

**Parameters:**
- `subscriber`: The subscriber address for the new subscription
- `plan_template_id`: The ID of the plan template to use

**Returns:** The unique subscription ID for the newly created subscription

**Authorization:** Requires subscriber authentication

**Errors:**
- `Error::NotFound`: If the plan template ID does not exist

### Retrieving a Plan Template

```rust
pub fn get_plan_template(
    env: Env,
    plan_template_id: u32,
) -> Result<PlanTemplate, Error>
```

**Parameters:**
- `plan_template_id`: The ID of the plan template to retrieve

**Returns:** The plan template details

**Errors:**
- `Error::NotFound`: If the plan template ID does not exist

## Benefits

### For Merchants

1. **Consistency**: All subscriptions created from a template have identical parameters
2. **Simplified Management**: Define plans once, use them many times
3. **Reduced Errors**: No risk of parameter input mistakes when creating subscriptions
4. **Standardization**: Enforce standard pricing and billing intervals

### For Subscribers

1. **Simplified Onboarding**: Subscribe with just one parameter (plan ID)
2. **Clear Offerings**: Understand exactly what they're subscribing to
3. **Consistency**: All subscribers on the same plan have the same terms

### For Developers

1. **Reduced Complexity**: Fewer parameters to manage during subscription creation
2. **Better UX**: Simpler API for common use cases
3. **Flexibility**: Direct subscription creation still available for custom cases

## Behavior and Guarantees

### Template Immutability and Versioning

The current implementation treats plan templates as **immutable**. Once created, plan templates cannot be updated. This design ensures:

- Existing subscriptions are not affected by template changes
- Subscribers know exactly what they're getting when they subscribe
- Historical records remain accurate

**If you need to change a plan**, create a new template with updated parameters and update your frontend/integration to point to the new template ID.

### Subscription Pinning to Template Version

When a subscription is created from a template using `create_subscription_from_plan()`, the subscription captures the template's parameters at creation time:

```
create_subscription_from_plan(subscriber, plan_template_id) 
  → Reads plan_template_id from storage
  → Extracts amount, interval_seconds, usage_enabled, merchant
  → Creates subscription with those exact parameters
  → Subscription is independent of the template thereafter
```

**Key behavior:** The subscription is **not** a reference to the template; it's a fully independent subscription with its own state. Once created:

- Changes to the template have **no effect** on the subscription (templates are immutable anyway)
- Subscriptions created before and after a template update have identical billing terms
- Each subscription maintains its own lifecycle independently

### Future Template Versioning (Not Yet Implemented)

If template updates are enabled in a future release, subscriptions would be pinned to the template version at creation time:

```
Scenario: Admin updates template from $9.99/month to $12.99/month

Subscriptions created BEFORE update:
  - Remain at $9.99/month (pinned to original version)
  - Continue billing at original rate indefinitely

Subscriptions created AFTER update:
  - Use new $12.99/month rate
  - Are independent subscription instances
```

The contract would track:

```rust
pub struct Subscription {
    // ... existing fields ...
    plan_template_id: Option<u32>,        // Which template was used
    template_version: u32,                // Version at creation time
    amount: i128,                         // Pinned amount (immutable)
    interval_seconds: u64,                // Pinned interval (immutable)
}
```

This ensures backwards compatibility: subscriptions are always pinned to their creation-time terms, even if the template evolves.

## Compatibility

### Backward Compatibility

The plan template feature is fully backward compatible:
- Direct subscription creation (`create_subscription`) continues to work unchanged
- Existing subscriptions are not affected
- No migration is required for existing deployments

### When to Use Templates vs. Direct Creation

**Use Plan Templates When:**
- Offering standard subscription tiers (Basic, Premium, Enterprise)
- Creating multiple subscriptions with identical parameters
- Simplifying the subscription process for end users
- Ensuring consistency across subscriptions

**Use Direct Creation When:**
- Creating one-off custom subscriptions
- Parameters vary significantly between subscriptions
- Flexibility is more important than consistency
- Prototyping or testing with non-standard parameters

## Examples

### Complete Workflow

```rust
// 1. Initialize the contract
client.init(&token, &admin, &1_000000i128);

// 2. Merchant creates plan templates
let basic_plan = client.create_plan_template(
    &merchant,
    &999i128,
    &2592000u64,
    &false
);

let premium_plan = client.create_plan_template(
    &merchant,
    &2999i128,
    &2592000u64,
    &true
);

// 3. Subscribers create subscriptions from plans
let alice_subscription = client.create_subscription_from_plan(
    &alice,
    &basic_plan
);

let bob_subscription = client.create_subscription_from_plan(
    &bob,
    &premium_plan
);

// 4. Subscribers deposit funds
client.deposit_funds(&alice_subscription, &alice, &10_000000i128);
client.deposit_funds(&bob_subscription, &bob, &20_000000i128);

// 5. Subscriptions are charged normally
client.charge_subscription(&alice_subscription);
client.charge_subscription(&bob_subscription);
```

### Multiple Subscribers on Same Plan

```rust
let plan_id = client.create_plan_template(
    &merchant,
    &1999i128,
    &2592000u64,
    &false
);

// Multiple subscribers can use the same plan
let sub1 = client.create_subscription_from_plan(&subscriber1, &plan_id);
let sub2 = client.create_subscription_from_plan(&subscriber2, &plan_id);
let sub3 = client.create_subscription_from_plan(&subscriber3, &plan_id);

// All subscriptions have identical parameters but independent state
```

## Testing

The plan template feature includes comprehensive test coverage:

- Template creation and retrieval
- Subscription creation from templates
- Multiple subscriptions from the same template
- Error handling for nonexistent templates
- Lifecycle operations (pause, resume, cancel) on template-based subscriptions
- Charging subscriptions created from templates
- Edge cases (zero amount, large amounts, different intervals)
- Compatibility with direct subscription creation

Run tests with:

```bash
cargo test -p subscription_vault
```

## Storage

Plan templates are stored in contract instance storage with the following key structure:

```rust
let key = (Symbol::new(env, "plan"), plan_template_id);
```

This ensures:
- Efficient lookup by plan ID
- Separation from subscription storage
- No conflicts with other contract data

## Future Enhancements

Potential future improvements to the plan template system:

1. **Template Updates**: Allow merchants to update templates (with versioning)
2. **Template Metadata**: Add name, description, and other metadata fields
3. **Template Deactivation**: Allow merchants to deactivate templates
4. **Template Discovery**: Query functions to list all templates for a merchant
5. **Template Analytics**: Track how many subscriptions use each template
6. **Template Inheritance**: Allow templates to inherit from other templates

## Conclusion

Plan templates provide a powerful way to standardize subscription offerings while maintaining flexibility for custom use cases. By reducing parameter input and ensuring consistency, they improve the developer experience and reduce the potential for errors in subscription management.

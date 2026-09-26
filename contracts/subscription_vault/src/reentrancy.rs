//! Reentrancy guard for fund-moving entrypoints.
//!
//! Uses a per-entrypoint storage flag in **persistent storage** for cross-contract safety.
//! The flag is set before any external token transfer and cleared
//! unconditionally on return (success or error).
//!
//! **CRITICAL SECURITY**: Uses persistent storage instead of instance storage to ensure
//! the lock is visible across cross-contract callback sequences. Instance storage may
//! not be atomically consistent during cross-contract calls in Soroban's invocation model.
//!
//! # Usage
//! ```ignore
//! let _guard = ReentrancyGuard::lock(&env, "deposit_funds")?;
//! // _guard is dropped at end of scope, releasing the lock
//! ```

use crate::types::Error;
use soroban_sdk::{Env, Symbol};

/// RAII guard that holds a reentrancy lock for the duration of a scope.
///
/// **CRITICAL**: Uses persistent storage to ensure cross-contract callback safety.
/// During cross-contract calls (e.g., `token.transfer()`), malicious contracts could
/// attempt to re-enter. Instance storage locks may not be visible during callbacks,
/// but persistent storage provides stronger consistency guarantees.
///
/// Acquiring the guard sets a per-entrypoint flag in persistent storage.
/// Dropping the guard clears it, even if the function returns an error.
pub struct ReentrancyGuard<'a> {
    env: &'a Env,
    key: Symbol,
}

impl<'a> ReentrancyGuard<'a> {
    /// Attempt to acquire the reentrancy lock for `entrypoint`.
    ///
    /// Returns `Err(Error::Reentrancy)` immediately if the lock is already
    /// held, indicating a reentrant call is in progress.
    ///
    /// **SECURITY**: Uses persistent storage for cross-contract callback protection.
    pub fn lock(env: &'a Env, entrypoint: &str) -> Result<Self, Error> {
        // Create Symbol key directly from entrypoint name
        // Symbols are limited to 32 characters, which is sufficient for our entrypoint names
        let key = Symbol::new(env, entrypoint);
        
        // Check if lock already exists in persistent storage (cross-contract safe)
        // Using persistent storage ensures the lock survives cross-contract callbacks
        if env.storage().persistent().has(&key) {
            return Err(Error::Reentrancy);
        }
        
        // Acquire lock in persistent storage with TTL extension
        // TTL ensures locks are eventually cleaned up even if drop() fails
        env.storage().persistent().set(&key, &true);
        
        // Extend TTL to 1 hour as safety measure for lock cleanup
        // Threshold=0 means extend immediately, extend_to=3600 means 1 hour from now
        env.storage().persistent().extend_ttl(&key, 0, 3600);
        
        Ok(Self { env, key })
    }
}

impl<'a> Drop for ReentrancyGuard<'a> {
    /// Release the lock unconditionally when the guard goes out of scope.
    ///
    /// **CRITICAL**: Ensures cleanup even during panics or cross-contract callback failures.
    /// The lock MUST be released to prevent permanently blocking the guarded function.
    fn drop(&mut self) {
        self.env.storage().persistent().remove(&self.key);
    }
}

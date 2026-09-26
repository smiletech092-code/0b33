# Stellabill Contracts — top-level Makefile
#
# Targets
# -------
# build    — cargo build (workspace, native)
# test     — cargo test  (workspace)
# verify   — run all Kani formal-verification harnesses
# check    — cargo check with kani_harness feature (compile-checks harnesses
#             without running Kani; useful in CI environments where Kani is not
#             installed)
# clean    — cargo clean

CARGO         ?= cargo
KANI          ?= cargo kani
CRATE         := -p subscription_vault

VERIFY_DIR    := contracts/subscription_vault/verification
KANI_HARNESSES := \
  $(VERIFY_DIR)/balance_non_negativity.rs \
  $(VERIFY_DIR)/interval_elapsed.rs \
  $(VERIFY_DIR)/auth_enforcement.rs

.PHONY: build test verify check clean

# ── Standard build / test ─────────────────────────────────────────────────────

build:
	$(CARGO) build --workspace

test:
	$(CARGO) test --workspace

# ── Kani formal verification ──────────────────────────────────────────────────
#
# Runs all harnesses in verification/ through Kani.  Each harness file is
# included directly via `--harness-file`; Kani's own Rust toolchain handles
# the `#[cfg(kani)]` gating automatically.
#
# To run a single harness use:
#   cargo kani -p subscription_vault \
#     --include-str contracts/subscription_vault/verification/balance_non_negativity.rs
#
# Prerequisite: install Kani with
#   cargo install --locked kani-verifier
#   cargo kani setup
#
verify:
	$(KANI) $(CRATE) \
	  --features kani_harness \
	  --output-format terse \
	  --harness balance_non_negativity::add_balance_non_negative_result \
	  --harness balance_non_negativity::sub_balance_preserves_non_negativity \
	  --harness balance_non_negativity::charge_cannot_overdraft \
	  --harness interval_elapsed::next_charge_time_monotone \
	  --harness interval_elapsed::next_charge_time_exact \
	  --harness interval_elapsed::interval_gate_admits_only_ready_charges \
	  --harness auth_enforcement::admin_is_always_authorized \
	  --harness auth_enforcement::operator_is_always_authorized \
	  --harness auth_enforcement::unauthorized_caller_is_always_rejected \
	  --harness auth_enforcement::auth_result_is_binary

# ── Harness compile-check (no Kani required) ──────────────────────────────────
#
# Compile-checks all harness files using the stable toolchain.  This is the
# fast CI gate: it catches type errors, missing imports, and API drift without
# needing the full Kani solver.
#
check:
	$(CARGO) check $(CRATE) --features kani_harness

# ── Clean ─────────────────────────────────────────────────────────────────────

clean:
	$(CARGO) clean

# Requirements Document

## Introduction

The `subscription_vault` contract (Stellarbill prepaid billing on Stellar) currently lacks two categories of documentation that external integrators and contract consumers need:

1. **ABI Changelog** — There is no `CHANGELOG.md` file at the repository root. Contract consumers have no documented way to learn about ABI-breaking changes across versions. Without a changelog, operators upgrading to a new contract version must diff source code manually to discover removed, renamed, or signature-changed entrypoints.

2. **Governance parameter documentation** — `docs/governance_proposals.md` describes the proposal lifecycle but omits the quorum threshold semantics, voting period length, what happens when quorum is not reached, and all governance-related default values baked into the code.

Both documents are purely additive; they do not change contract code or on-chain state.

## Glossary

- **ABI (Application Binary Interface)**: The set of public entrypoints, their argument types, return types, and error codes exposed by the compiled contract. Any change visible to a caller constitutes an ABI change.
- **Entrypoint**: A `pub fn` declared inside the `#[contractimpl]` block in `lib.rs` that is callable on-chain.
- **Semantic Versioning (SemVer)**: A versioning scheme where a version number takes the form `MAJOR.MINOR.PATCH`. A `MAJOR` bump signals breaking changes; `MINOR` signals backward-compatible additions; `PATCH` signals backward-compatible fixes.
- **Quorum**: The minimum weighted vote total (expressed in basis points of total guardian weight) required for a governance proposal to be eligible for execution.
- **Guardian**: An address with an assigned voting weight that participates in governance proposals.
- **ETA (Execution Time After)**: The earliest on-chain timestamp at which a proposal may be executed after its quorum window closes.
- **Timelock**: The period between proposal submission and the ETA, during which guardians may vote.
- **Basis Points (bps)**: One hundredth of a percent; 10 000 bps = 100%.
- **CHANGELOG.md**: A markdown file at the repository root that records notable changes, grouped by release version, following the [Keep a Changelog](https://keepachangelog.com) convention.
- **Storage_Version**: The integer constant (`STORAGE_VERSION`) in `lib.rs` that tracks the on-chain storage schema version; currently `5`.
- **Contract_Version**: The `version` field in `contracts/subscription_vault/Cargo.toml`; currently `0.1.0`.
- **Changelog**: Short for `CHANGELOG.md`.
- **Governance_Doc**: Short for `docs/governance_proposals.md`.

---

## Requirements

### Requirement 1: CHANGELOG.md — Initial File Creation

**User Story:** As a contract consumer or integration engineer, I want a `CHANGELOG.md` at the repository root, so that I can learn which ABI entrypoints were added, changed, deprecated, or removed in each release without reading source diffs.

#### Acceptance Criteria

1. THE Changelog SHALL exist at the path `CHANGELOG.md` in the repository root.
2. THE Changelog SHALL include a header section that describes its purpose: tracking ABI-breaking and ABI-additive changes across contract versions.
3. THE Changelog SHALL include a link or reference to the semantic versioning specification so readers understand how version numbers are assigned.
4. THE Changelog SHALL include a `[Unreleased]` section for changes not yet assigned a version tag.

---

### Requirement 2: CHANGELOG.md — Version Entry Format

**User Story:** As a contract consumer, I want each version entry in the changelog to use a consistent, machine-readable format, so that I can scan for breaking changes quickly.

#### Acceptance Criteria

1. THE Changelog SHALL represent each released version as a second-level heading of the form `## [MAJOR.MINOR.PATCH] - YYYY-MM-DD`.
2. WHEN a version entry contains ABI changes, THE Changelog SHALL group those changes under one or more of the following sub-headings: `### Added`, `### Changed`, `### Deprecated`, `### Removed`.
3. THE Changelog SHALL include a `### Added` sub-section listing every entrypoint introduced in that version.
4. THE Changelog SHALL include a `### Changed` sub-section listing every entrypoint whose argument list, return type, or error set changed in that version.
5. THE Changelog SHALL include a `### Deprecated` sub-section listing every entrypoint that is scheduled for removal in a future version.
6. THE Changelog SHALL include a `### Removed` sub-section listing every entrypoint that was deleted in that version.
7. WHEN a sub-section is empty for a given version, THE Changelog SHALL omit that sub-section rather than include an empty heading.

---

### Requirement 3: CHANGELOG.md — Seed Entry for v0.1.0

**User Story:** As a contract consumer integrating for the first time, I want a baseline entry for the initial contract version, so that I have a complete ABI inventory as a starting reference.

#### Acceptance Criteria

1. THE Changelog SHALL contain a version entry for `[0.1.0]`.
2. THE `[0.1.0]` entry SHALL include a `### Added` sub-section listing the initial set of ABI entrypoints.
3. THE `[0.1.0]` Added section SHALL enumerate entrypoints grouped by functional area: Initialisation & Config, Operator Management, Emergency Stop, Subscription Lifecycle, Plan Templates, Coupons & Discounts, Charging, Merchant Operations, Governance Proposals, Guardian Management, Blocklist, and Queries & Exports.
4. THE `[0.1.0]` entry SHALL note the initial `Storage_Version` value (`5`) so consumers know which schema they are deploying against.
5. THE `[0.1.0]` entry SHALL note the `soroban-sdk` version dependency (`22.0.0`).

---

### Requirement 4: CHANGELOG.md — ABI-Breaking Change Notation

**User Story:** As a contract consumer performing an upgrade, I want breaking changes to be clearly marked, so that I can prioritise my integration work.

#### Acceptance Criteria

1. THE Changelog SHALL prefix every entry in `### Removed` or `### Changed` that breaks binary compatibility with the tag `[BREAKING]`.
2. THE Changelog SHALL define what constitutes a breaking ABI change in its introductory section: removing an entrypoint, changing argument count or types, changing the return type, adding a non-optional argument, or changing the `DataKey` discriminant of a persistent storage key.
3. WHERE a breaking change also requires an on-chain migration, THE Changelog SHALL reference the relevant migration entrypoint or runbook doc.

---

### Requirement 5: CHANGELOG.md — Storage Version Tracking

**User Story:** As an operator running the migration script, I want the changelog to record storage schema bumps alongside ABI changes, so that I know when I need to call `migrate` before upgrading.

#### Acceptance Criteria

1. WHEN a version entry includes a storage schema bump, THE Changelog SHALL include a `### Migration` sub-section describing the `Storage_Version` change (old → new) and the migration entrypoint to invoke.
2. THE `[0.1.0]` entry SHALL document that `STORAGE_VERSION = 5` is the baseline (no prior migration needed for a fresh deploy).

---

### Requirement 6: Governance Doc — Quorum Threshold Documentation

**User Story:** As a guardian or protocol operator, I want `docs/governance_proposals.md` to state the quorum threshold semantics precisely, so that I can predict whether a set of votes will pass before calling `execute_proposal`.

#### Acceptance Criteria

1. THE Governance_Doc SHALL include a section titled "Governance Parameters" that documents all configurable governance parameters.
2. THE Governance_Doc SHALL state that `quorum_bps` is expressed in basis points (0–10 000), where 10 000 means 100% of total guardian weight must vote yes.
3. THE Governance_Doc SHALL document the formula used to compute the required vote count: `required_votes = floor(total_weight × quorum_bps / 10 000)`.
4. THE Governance_Doc SHALL state the valid range for `quorum_bps` at submission time: 0 ≤ `quorum_bps` ≤ 10 000; values above 10 000 are rejected with `Error::InvalidInput`.
5. THE Governance_Doc SHALL clarify that `quorum_bps = 0` means the proposal passes with zero yes-votes (no guardian approval required).
6. THE Governance_Doc SHALL include a worked example showing the quorum calculation for a concrete guardian set (e.g., three guardians each with weight 100, quorum_bps = 6 700).

---

### Requirement 7: Governance Doc — Voting Period and ETA Documentation

**User Story:** As a guardian, I want the governance doc to explain the ETA and timelock mechanics precisely, so that I know the deadline by which I must cast my vote.

#### Acceptance Criteria

1. THE Governance_Doc SHALL state that there is no protocol-enforced minimum or maximum ETA duration; the ETA value is chosen by the proposal submitter at submission time.
2. THE Governance_Doc SHALL state that the ETA must be strictly greater than the current ledger timestamp at the time of `submit_proposal`; proposals with `eta ≤ now` are rejected with `Error::InvalidInput`.
3. THE Governance_Doc SHALL state that votes are locked once `now >= proposal.eta`; any `vote_proposal` call after the ETA returns `Error::InvalidInput` and emits a `VoteLockedEvent`.
4. THE Governance_Doc SHALL state that `execute_proposal` requires `now >= proposal.eta`; calls before the ETA return `Error::InvalidInput`.
5. THE Governance_Doc SHALL include a timeline diagram or ASCII illustration showing the relationship between submission, voting window, ETA, and execution.

---

### Requirement 8: Governance Doc — Quorum-Not-Reached Outcome

**User Story:** As a protocol operator, I want the governance doc to state what happens when quorum is not reached at execution time, so that I know how to handle failed proposals.

#### Acceptance Criteria

1. THE Governance_Doc SHALL state that if `votes_for < required_votes` at execution time, `execute_proposal` returns `Error::InvalidInput` without mutating any state or marking the proposal as executed.
2. THE Governance_Doc SHALL state that a failed execution attempt does not prevent future execution attempts; the proposal remains open until cancelled or its storage TTL expires.
3. THE Governance_Doc SHALL state that the admin may call `cancel_proposal` at any time to permanently close a proposal that will not reach quorum.
4. THE Governance_Doc SHALL state the storage TTL policy for proposals: persistent storage with a 30-day TTL extension threshold and a 365-day target TTL.

---

### Requirement 9: Governance Doc — Default Values Table

**User Story:** As a developer deploying the contract, I want a table of all governance-related default values, so that I can configure guardians and proposals with predictable starting parameters.

#### Acceptance Criteria

1. THE Governance_Doc SHALL include a "Default Values" table with at least the following rows: initial guardian count (0), initial guardian total weight (0), `NextProposalId` initial value (0), storage TTL threshold for proposals (30 days), storage TTL target for proposals (365 days), and storage TTL settings for guardians (30-day threshold, 365-day target).
2. THE Governance_Doc SHALL state that the contract starts with no guardians, meaning any `submit_proposal`/`vote_proposal` call in single-admin mode can be submitted with `quorum_bps = 0` and executed immediately after the ETA.
3. THE Governance_Doc SHALL note that `ProposalKind::UpgradeContract` is reserved and returns `Error::InvalidInput` when executed; it is not yet implemented.
4. THE Governance_Doc SHALL document the `VoteLockedEvent` fields alongside the other governance events.

---

### Requirement 10: Governance Doc — Guardian Removal and Vote Invalidation

**User Story:** As a security auditor, I want the governance doc to precisely describe what happens to in-flight votes when a guardian is removed mid-proposal, so that I can verify the quorum logic is not gameable.

#### Acceptance Criteria

1. THE Governance_Doc SHALL state that guardian removal takes effect immediately; removed guardians cannot cast new votes on any proposal.
2. THE Governance_Doc SHALL state that at `execute_proposal` time the quorum check re-reads current guardian weights; votes cast by a guardian who was subsequently removed are excluded from both `votes_for` and `votes_against`.
3. THE Governance_Doc SHALL state that `total_weight` used in the quorum formula is the sum of currently registered guardian weights at execution time, not at submission time.
4. THE Governance_Doc SHALL include an example showing that removing a guardian who already voted yes can cause a previously-passing proposal to fail quorum.

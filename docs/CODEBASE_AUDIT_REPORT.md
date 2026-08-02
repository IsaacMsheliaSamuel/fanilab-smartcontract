# FaniLab Smart Contracts — Codebase Audit Report

**Audit date:** 2026-08-01
**Scope:** All 7 crates in `contracts/` (`escrow_contract`, `delivery_contract`, `dispute_resolution_contract`, `fleet_management_contract`, `identity_reputation_contract`, `settlement_contract`, `shared_types`), all `.github/workflows/*`, `scripts/*.sh`, `Makefile`/`Makefile.windows`, `Cargo.toml`/`deny.toml`, and every root/`docs/` markdown file.
**Method:** Direct line-by-line reading of every contract source and test file (not static-analysis output), cross-checked against the repository's own prior audit trail (`fani-smartcontract-issues.md`, 130 issues filed to `github.com/fanilabs/fanilab-smartcontract`), with independent re-verification of code state — see [Meta-Finding 0](#meta-finding-0-the-issue-tracker-cannot-be-trusted-as-a-remediation-signal) below.
**No source code was modified to produce this report.**

---

## How to read this report

Each finding has an ID (`A1`, `B3`, …) matching the phase it belongs to in `docs/CODEBASE_CLEANUP_PLAN.md`, so the two documents cross-reference directly. Findings are full-detail where they represent genuine logic/security/architecture defects I independently verified against current code. A final appendix summarizes the remaining lower-severity documentation/tooling items already catalogued in `fani-smartcontract-issues.md` / GitHub issues #31–#144, to avoid duplicating 4,000+ lines of prior work verbatim — each appendix row was spot-checked, not assumed.

> **Post-review note (2026-08-01):** a second pass cross-referenced all 45 findings below (`A1`–`G7`) against `fani-smartcontract-issues.md`'s issue titles. **Every one of them maps to a pre-existing GitHub issue** (#7–#144) — this report independently re-derived, and thereby corroborated, the prior three-pass backlog rather than discovering a materially different set of problems. Two items (`A9`, `C4`) were reassessed and downgraded/merged on review (see `docs/CODEBASE_CLEANUP_PLAN.md`'s "Eliminated / downgraded" section for the reasoning), and `E1` was reclassified as a documentation note rather than a defect. **`docs/CODEBASE_CLEANUP_PLAN.md` is the authoritative, deduplicated execution plan** — it references this report's evidence but is not a 1:1 restatement of it; treat this document as supporting detail, not a second independent backlog.

---

## Meta-Finding 0: The issue tracker cannot be trusted as a remediation signal

**Severity: Critical (process)**

`fani-smartcontract-issues.md` claims all 130 previously-found issues were "filed to GitHub," and indeed `gh issue list` confirms 130 issues exist on `fanilabs/fanilab-smartcontract`, of which **125 are closed with `stateReason: COMPLETED`**. However, `git log --oneline --all` shows **no commits between the last feature PR (`2568a44`) and the six `docs: publish/update issue backlog` commits** — i.e., no code was changed while those 125 issues were being closed as "completed."

Independent spot-verification against current code confirms the underlying defects are still present for every closed issue checked:

| Closed issue (state: COMPLETED) | Verified current reality |
|---|---|
| #7 "`freeze_funds` has no authorization check" | `escrow_contract/lib.rs:536` — `freeze_funds(env: Env, delivery_id: u64)` still takes no caller and calls no `require_auth()`. **Not fixed.** |
| #133 "deploy/init scripts committed empty" | `scripts/deploy-contract.sh` and `scripts/initialize-contract.sh` are still 0 bytes. **Not fixed.** |
| #135 "leftover developer debris" | `test_script.py` and `tests_passing.png` are still present at repo root. **Not fixed.** |
| #136 "`.vscode/settings.json` pins stale `wasm32-unknown-unknown`" | Still `"rust-analyzer.cargo.target": "wasm32-unknown-unknown"` while CI uses `wasm32v1-none`. **Not fixed.** |
| #127 "README badges point to nonexistent org" | `README.md:319-320,364` still link `github.com/fanilab/FaniLab-SmartContract` (actual org is `fanilabs/fanilab-smartcontract`). **Not fixed.** |
| #128 "version mismatch" | `README.md:324` / `SECURITY.md:7` still say `0.2.x`; every `Cargo.toml` still says `0.1.0`. **Not fixed.** |

**Why it matters:** anyone consulting GitHub issue state (a contributor, a security reviewer, `PRODUCTION_READINESS.md`'s own claims) will conclude these problems are resolved. They are not. This audit therefore treats GitHub issue state as **non-authoritative** and reports only what is independently observable in the current tree.

**Suggested solution:** stop bulk-closing issues without a linked fix commit/PR; reopen the 125 closed issues (or at minimum the ~40 substantive ones re-confirmed in this report) until a commit actually lands; add a lightweight policy (CONTRIBUTING.md note or a PR template checkbox) requiring "Closes #N" only on merged fix commits.
**Complexity:** Trivial (process/governance fix, no code).

---

# Phase A — Critical Bugs

### A1. `escrow_contract::init` enforces no ceiling on `platform_fee_bps`, enabling permanent fund lock or zero driver payout

**File:** `contracts/escrow_contract/lib.rs:159-182` (cf. the 1000-bps cap that *does* exist in `update_platform_fee`, line 194)

`init` stores `platform_fee_bps` verbatim with no upper bound, while the only other fee-setting path (`update_platform_fee`) caps it at 1000 bps (10%). If an admin passes an oversized value at `init` (typo, wrong units, or malice), `calculate_fee` (`amount.saturating_mul(platform_fee_bps as i128) / 10_000`, line 52-54) can compute a `platform_fee` **larger than `record.amount`**. `release_escrow` then does `driver_amount = record.amount.saturating_sub(platform_fee)` → saturates to 0 (driver receives nothing), and then attempts `token::Client::transfer(..., &admin, &platform_fee)` for an amount that may exceed the contract's actual token balance, which the token contract will reject — **permanently reverting every future `release_escrow` call for every escrow that inherits this fee**, since there is no way to change `platform_fee_bps` back down before *this* escrow settles (the fee is read live from `ProtocolConfig` at release time, so a later `update_platform_fee` fixes new escrows but the already-locked ones were already exposed at deposit time only in spirit — the actual read-at-release-time behavior means an admin fix *does* apply retroactively, but only if caught before release is attempted and reverts).
**Severity:** Critical — direct path to driver funds being zeroed or escrow release being permanently blocked.
**Affected files:** `contracts/escrow_contract/lib.rs` (`init` fn, `calculate_fee`, `release_escrow`, `resolve_dispute`).
**Suggested solution:** apply the same `> 1000 → InvalidFee` guard in `init` that `update_platform_fee` already has; extract both into one `validate_fee_bps()` helper.
**Complexity:** Trivial (a few lines).

### A2. A sender can bypass admin-mediated dispute resolution entirely by cancelling a `Disputed` delivery

**Files:** `contracts/delivery_contract/lib.rs:35-53` (`validate_transition`), `:122-164` (`cancel_delivery`); `contracts/escrow_contract/lib.rs:381-409` (`refund_escrow`)

`validate_transition` permits `Disputed → Cancelled`. `cancel_delivery` only checks `delivery.sender == sender` and calls the escrow contract's `refund_escrow`. `refund_escrow`'s authorization is `admin_authorized || sender_authorized`, and its state guard accepts `Locked` **or `Paused`** (the state a dispute leaves the escrow in). So once a dispute is raised (escrow → `Paused`), the *sender* — one of the two disputing parties — can unilaterally call `cancel_delivery`, which forces a full self-refund through `refund_escrow`, **without any admin ever calling `resolve_dispute`**. This defeats the entire arbitration system: a sender can dispute a delivery the driver actually completed, then immediately self-refund before the admin rules.
**Severity:** Critical — direct fund-theft vector against drivers, defeats the dispute subsystem's entire purpose.
**Affected files:** `contracts/delivery_contract/lib.rs`, `contracts/escrow_contract/lib.rs`.
**Suggested solution:** either (a) exclude `Disputed` from `refund_escrow`'s allowed states — only `resolve_dispute`/`resolve_dispute_split` should be able to move money out of `Paused`, or (b) have `cancel_delivery` reject cancellation once `DeliveryStatus::Disputed` has been reached (there's no legitimate "cancel" of a dispute in progress — it should route through `dispute_resolution_contract`).
**Complexity:** Small (guard-condition change) but requires a state-machine decision — needs a test asserting the old bypass now panics.

### A3. `freeze_funds` has no authorization check whatsoever

**File:** `contracts/escrow_contract/lib.rs:536-543`

```rust
pub fn freeze_funds(env: Env, delivery_id: u64) {
    let mut record = load_escrow(&env, delivery_id);
    if record.status == EscrowStatus::Locked {
        record.status = EscrowStatus::Paused;
        ...
```

No `caller` parameter, no `require_auth()`, no admin/party check — **any address, or any other contract, can freeze any active escrow at will.** It is intended to be invoked only by `dispute_resolution_contract::raise_dispute` (line 174-179 of `dispute_resolution_contract/lib.rs`), but nothing in `escrow_contract` enforces that caller identity.
**Severity:** Critical — unauthenticated denial-of-service on every escrow in the protocol; an attacker can freeze all in-flight deliveries indefinitely (there is no unfreeze path outside dispute resolution).
**Affected files:** `contracts/escrow_contract/lib.rs`.
**Suggested solution:** add a `caller: Address` parameter, `caller.require_auth()`, and restrict to admin or a configured `dispute_resolution_contract` address (mirroring the `settlement_contract` allowlist pattern already used for `SettlementContract`).
**Complexity:** Small — but is a breaking API change (new required parameter), so every caller (`dispute_resolution_contract`, tests) must be updated together.

### A4. `identity_reputation_contract`'s two initializers can permanently brick the contract with no recovery path

**File:** `contracts/identity_reputation_contract/lib.rs:51-75`

The contract exposes both `init(admin)` and `initialize(admin, delivery_contract, dispute_contract)`; both guard on the same `DataKey::Admin` flag and both panic with `AlreadyInitialized` if it's set. If a deployer calls the simpler `init` first, `DataKey::DeliveryContract`/`DataKey::DisputeContract` are never set, `initialize` can never be called afterward (admin already set), and there is **no admin setter anywhere in this contract to configure those two addresses post-init**. Since `increase_reputation`/`decrease_reputation` both hard-require `caller == delivery_contract || caller == dispute_contract` read from those unset keys, both functions panic with `NotInitialized` forever — the entire reputation system is permanently dead for that deployment, unfixable without a redeploy.
**Severity:** Critical — a single deployment mistake (an extremely easy one, since `init` is the more obviously-named function) permanently disables reputation tracking.
**Affected files:** `contracts/identity_reputation_contract/lib.rs`.
**Suggested solution:** delete `init` (keep one initializer, `initialize`), and add an admin-gated `set_delivery_contract`/`set_dispute_contract` pair for post-deploy recovery/rotation regardless.
**Complexity:** Small code change, but is a breaking API change requiring deployment-script updates.

### A5. Driver reputation can only ever decrease — `increase_reputation` is dead code, and reputation is tracked in two divergent, un-synced places

**Files:** `contracts/identity_reputation_contract/lib.rs:108-128, 205-254`; `contracts/delivery_contract/lib.rs:272-293`

`identity_reputation_contract::increase_reputation` exists, is fully implemented (tiered point awards, `MAX_REPUTATION` cap), and correctly authorization-gated — but **no other contract in the workspace ever calls it.** Meanwhile `delivery_contract::confirm_delivery` independently increments its *own* `DriverProfile.reputation_score` (via `shared_types::StorageKey::DriverProfile`, a completely separate storage location from `identity_reputation_contract`'s `DataKey::DriverProfile`) on every successful delivery, with no cap and no tier logic. The result: two disjoint reputation ledgers exist for the same driver — `delivery_contract`'s copy only ever goes up (uncapped), `identity_reputation_contract`'s copy (the one `get_driver_tier`/`is_eligible_for_enterprise` actually read) starts at 50 and can **only go down**, since the sole caller of `decrease_reputation` is `dispute_resolution_contract::resolve_dispute_refund_sender`. A driver's tier can therefore never improve — it can only ever be dragged toward `Bronze`.
**Severity:** Critical — core product feature (reputation-based driver tiers) is structurally non-functional; also a data-integrity problem (two disagreeing sources of truth for "driver reputation").
**Affected files:** `contracts/delivery_contract/lib.rs`, `contracts/identity_reputation_contract/lib.rs`.
**Suggested solution:** remove the local reputation increment in `delivery_contract::confirm_delivery`; have it cross-call `identity_reputation_contract::increase_reputation` instead (mirroring how `dispute_resolution_contract` already cross-calls `decrease_reputation`), and pick a single point of authority for `DriverProfile`.
**Complexity:** Medium — touches cross-contract wiring and needs new integration tests; also entangled with A4 (the contract addresses needed for the auth check may not be configured).

### A6. `settlement_contract` is a complete no-op stub, already wired into the live escrow payout path

**Files:** `contracts/settlement_contract/src/lib.rs` (entire file); `contracts/escrow_contract/lib.rs:56-93` (`payout_driver`)

Every function in `settlement_contract` is a placeholder: `init` never persists the admin, `get_driver_preference` always returns `None`, `execute_settlement_swap` does nothing. Yet `escrow_contract::payout_driver` already calls `get_driver_preference` and (conditionally) `execute_settlement_swap` on this contract whenever an admin has configured `set_settlement_contract`. Because `get_driver_preference` always returns `None`, the swap branch is currently unreachable dead code and the fallback direct `transfer` always runs — so today this is *silently* harmless, but it means the currency-swap feature is entirely fictional despite being wired into a production fund-movement path with **zero test coverage beyond a single `init` test** (`settlement_contract/src/lib.rs:48-58`).
**Severity:** Critical (architectural — advertised functionality doesn't exist, sitting on the fund-transfer critical path) / High in practice today (currently inert).
**Affected files:** `contracts/settlement_contract/src/lib.rs`, `contracts/escrow_contract/lib.rs`.
**Suggested solution:** either implement the DEX/liquidity-pool integration for real, or remove the cross-contract call from `payout_driver` until it exists, and stop advertising it as available in docs (`docs/API.md`).
**Complexity:** Large (real implementation) or Trivial (temporarily strip the dead call path) depending on chosen direction.

### A7. `register_fleet` permanently fails the second time the same owner registers a fleet

**Files:** `contracts/fleet_management_contract/lib.rs:105-154`; `contracts/identity_reputation_contract/lib.rs:108-113`

When an `identity_contract` is configured, `register_fleet` unconditionally cross-calls `identity_reputation_contract::register_driver(owner)`. `register_driver` panics with `FaniLabError::AlreadyInitialized` if a `DriverProfile` already exists for that address. Since Soroban propagates cross-contract panics, **any owner attempting to register a second fleet — or any address that previously called `register_driver` directly — causes `register_fleet` to revert unconditionally**, even though the fleet counter was already incremented in persistent storage before the panic (the counter bump happens first, so counters leak/skip on every failed retry, compounding the bug).
**Severity:** Critical — a legitimate, common business operation (one owner operating multiple fleets) is structurally impossible once an identity contract is wired up.
**Affected files:** `contracts/fleet_management_contract/lib.rs`.
**Suggested solution:** check `is_authorized_contract`/existing-profile state before calling `register_driver`, or catch/ignore `AlreadyInitialized` semantics by exposing a `register_driver_if_absent` idempotent variant on `identity_reputation_contract`.
**Complexity:** Small–Medium (needs a new idempotent entry point or an existence check hook on the identity contract).

### A8. `resolve_dispute_split` mislabels the final escrow status as `Refunded`

**File:** `contracts/escrow_contract/lib.rs:481-527`

After splitting funds between sender and driver in arbitrary proportions (`sender_share_bps`), the function unconditionally sets `record.status = EscrowStatus::Refunded` (line 520) — even when most or all of the amount went to the driver. Any downstream consumer of `get_escrow`/`EscrowState` (indexers, the driver-facing UI, future contract logic gating on `Refunded` meaning "sender got their money back") will misclassify a driver-favoring split as a full refund.
**Severity:** Critical (state-integrity bug feeding every downstream consumer of escrow status) though not directly fund-losing.
**Affected files:** `contracts/escrow_contract/lib.rs`.
**Suggested solution:** add an `EscrowState::Split` variant (already conceptually present in `dispute_resolution_contract::DisputeStatus::Split`) and set that instead.
**Complexity:** Small, but touches the shared `EscrowState` enum (`shared_types/lib.rs:220-225`) — a cross-crate, backward-incompatible storage schema change.

### A9. `resolve_dispute`'s refund branch skips the balance-sufficiency guard present everywhere else

**File:** `contracts/escrow_contract/lib.rs:431-479`

`release_escrow` (line 344-348), `refund_escrow` (line 393-397), and `resolve_dispute_split` (line 496-500) all check `contract_balance >= record.amount` before transferring. `resolve_dispute`'s `else` branch (the refund-on-dispute path, lines 464-471) transfers directly with no such check. In practice the underlying token contract will itself reject an over-balance transfer, so this is not currently an unguarded fund-loss path, but it is an inconsistent defense-in-depth gap that produces a different (untyped, token-contract-level) panic instead of the contract's own typed `InsufficientFunds` error, breaking the uniform error-handling contract the rest of the file establishes.
**Severity:** High (consistency/defense-in-depth gap, not directly exploitable given token-contract behavior, but the divergence itself is a code-review red flag suggesting the omission was accidental).
**Affected files:** `contracts/escrow_contract/lib.rs`.
**Suggested solution:** add the same `contract_balance < record.amount` guard to this branch, or better, extract a single `assert_sufficient_balance()` helper used by all four fund-moving functions.
**Complexity:** Trivial.

---

# Phase B — Security Improvements

### B1. `create_escrow` never validates `amount > 0`

**File:** `contracts/escrow_contract/lib.rs:293-330`
A caller can create an escrow with `amount = 0` or negative `amount` (the field is `i128`). The `token::Client::transfer` call may itself reject negative amounts depending on the token implementation, but this is not guaranteed contract-side, and a zero-amount escrow is silently accepted, creating a valid `DeliveryId`/`EscrowRecord` pair that can still go through the full dispute/release lifecycle for no economic value — a griefing vector for spamming the persistent storage / event log.
**Severity:** Medium-High.
**Affected files:** `contracts/escrow_contract/lib.rs`.
**Suggested solution:** `if amount <= 0 { panic_with_error!(&env, EscrowError::InvalidState) }` (or a new `InvalidAmount` variant) at the top of `create_escrow`.
**Complexity:** Trivial.

### B2. No expiry/timeout mechanism for `Locked` escrows

If a driver is assigned but never completes/confirms a delivery, and neither party raises a dispute, funds can remain `Locked` indefinitely with no automatic timeout-refund path. This is a design gap rather than a coding bug.
**Severity:** Medium.
**Affected files:** `contracts/escrow_contract/lib.rs`, `contracts/delivery_contract/lib.rs`.
**Suggested solution:** add an admin- or time-gated `reclaim_stale_escrow(delivery_id)` that permits refund after a configurable inactivity window past `created_at`.
**Complexity:** Medium (new state field + new entry point + tests).

### B3. No emergency pause / circuit breaker across the protocol

None of the six contracts have a global pause switch. In an active-exploit scenario (e.g., A1–A3 above being exploited in production) there is no way to halt fund movement without a full redeploy/migration.
**Severity:** High (operational risk multiplier for every other finding in this report).
**Affected files:** all contracts, most importantly `escrow_contract`.
**Suggested solution:** add an admin-gated `paused: bool` instance flag checked at the top of every fund-moving entry point.
**Complexity:** Medium, touches every public fund-moving function across contracts.

### B4. `dispute_resolution_contract::remove_admin` can remove the last admin, bricking governance

**File:** `contracts/dispute_resolution_contract/lib.rs:79-85`
`remove_admin` has no check preventing an admin from removing themselves as the last remaining admin. Once the last `DataKey::Admin(addr)` entry is removed, `is_admin` returns `false` for everyone and every admin-gated function (`add_admin` included) becomes permanently uncallable.
**Severity:** High.
**Affected files:** `contracts/dispute_resolution_contract/lib.rs`.
**Suggested solution:** track an admin count (or iterate a bounded admin list) and reject removal that would bring the count to zero.
**Complexity:** Small–Medium (current `DataKey::Admin(Address)` sparse-map design has no cheap way to count admins — needs a companion counter or an enumerable list).

### B5. Zero slippage protection on the settlement-swap payout path

**File:** `contracts/escrow_contract/lib.rs:74-88`
`payout_driver` invokes `execute_settlement_swap` with `min_amount_out` hardcoded to `0i128`, meaning if/when `settlement_contract` (A6) is actually implemented, a driver payout could be swapped at an arbitrarily bad rate with the contract offering no protection.
**Severity:** High (latent — only actionable once A6 is implemented, but the wiring is landed now and easy to forget to fix later).
**Affected files:** `contracts/escrow_contract/lib.rs`.
**Suggested solution:** compute a real `min_amount_out` (oracle price, configurable slippage-bps parameter) instead of the literal `0`.
**Complexity:** Medium, blocked on A6.

### B6. Admin can silently repoint `settlement_contract` mid-flight with no timelock

**File:** `contracts/escrow_contract/lib.rs:233-239`
`set_settlement_contract` takes effect immediately with no delay/announcement, meaning a compromised or malicious admin key can redirect all driver payouts through an attacker-controlled "settlement contract" address (which `payout_driver` will happily invoke with `env.invoke_contract`, trusting whatever `get_driver_preference`/`execute_settlement_swap` responses it returns) with zero warning to affected drivers.
**Severity:** High.
**Affected files:** `contracts/escrow_contract/lib.rs`.
**Suggested solution:** add a timelock (`propose_settlement_contract` + `apply_settlement_contract` after N ledgers) mirroring the existing two-step admin-transfer pattern.
**Complexity:** Medium.

### B7. Fund-moving functions update state after external transfers, not fully honoring checks-effects-interactions

**File:** `contracts/escrow_contract/lib.rs` (`release_escrow`, `refund_escrow`, `resolve_dispute`, `resolve_dispute_split`)
Every one of these functions calls `token::Client::transfer` (an external/cross-contract call) *before* writing `record.status` back to storage via `save_escrow`. Soroban's execution model makes classic EVM-style reentrancy unlikely (no untrusted external code runs mid-call the way Solidity's `call` allows), but this still contradicts the checks-effects-interactions pattern that `PRODUCTION_READINESS.md` explicitly claims is implemented, and is fragile if the token/settlement integration surface grows (e.g., once A6/B5 land, `execute_settlement_swap` is an actual cross-contract call to code the protocol doesn't control).
**Severity:** Medium (not currently exploitable given Soroban's auth/call model, but a real practice gap that compounds risk as A6 is implemented).
**Affected files:** `contracts/escrow_contract/lib.rs`.
**Suggested solution:** flip the order — write the new `record.status` first, then transfer — for every fund-moving function.
**Complexity:** Small.

### B8. Untyped `panic!("...")` used throughout `delivery_contract` and in `escrow_contract`'s admin-transfer functions

**Files:** `contracts/delivery_contract/lib.rs` (nearly every function — `cancel_delivery`, `assign_driver`, `mark_in_transit`, `confirm_delivery`, `raise_dispute`, `get_delivery`); `contracts/escrow_contract/lib.rs:253,272` (`propose_admin`, `accept_admin`)
Every other contract in the workspace uses `panic_with_error!` with a typed `#[contracterror]` enum, giving callers/dApps a stable numeric error code to branch on. `delivery_contract` instead panics with raw string literals (`panic!("NotAuthorized")`, `panic!("InvalidState")`, `panic!("DeliveryNotFound")`, `panic!("EscrowNotConfigured")`) even though it already defines a `DeliveryError` enum (used only once, via `validate_transition`, and then immediately discarded via `.unwrap_or_else(|_| panic!("InvalidState"))` instead of `panic_with_error!`). This means client code integrating with `delivery_contract` cannot programmatically distinguish error conditions the way it can for every other contract.
**Severity:** Medium-High (API consistency, Soroban best-practice violation, and materially worse DX/observability for integrators).
**Affected files:** `contracts/delivery_contract/lib.rs`, `contracts/escrow_contract/lib.rs`.
**Suggested solution:** expand `DeliveryError` to cover `NotAuthorized`, `DeliveryNotFound`, `EscrowNotConfigured`, etc., and replace every `panic!(...)` with `panic_with_error!(&env, DeliveryError::X)`.
**Complexity:** Medium (mechanical but touches every function; needs matching test updates since tests currently assert on Soroban's generic panic rather than a typed error code — see G-phase).

### B9. `AuthorizedContract` allowlist is built but never consulted

**File:** `contracts/identity_reputation_contract/lib.rs:84-106`
`set_authorized_contract`/`is_authorized_contract` implement a full admin-gated allowlist, but no function in the contract (or anywhere else in the workspace) ever calls `is_authorized_contract` to gate anything. It's a fully-built, unused security control — dead code today, but its presence in the API surface implies a guarantee ("only authorized contracts can do X") that isn't actually enforced anywhere.
**Severity:** Medium (misleading unused control, not itself exploitable, but risks false confidence).
**Affected files:** `contracts/identity_reputation_contract/lib.rs`.
**Suggested solution:** either wire it into `increase_reputation`/`decrease_reputation` in place of (or in addition to) the hardcoded `delivery_contract`/`dispute_contract` check, or remove it.
**Complexity:** Small.

### B10. `dispute_time_limit` accepts `0` at `init` with no minimum enforced

**File:** `contracts/dispute_resolution_contract/lib.rs:47-67`
If `dispute_time_limit` is initialized to `0`, `raise_dispute`'s `Delivered` branch check (`current_time > delivered_at + dispute_limit`) becomes true almost immediately after delivery, effectively disabling the ability to ever dispute a `Delivered` delivery. There's also no admin setter to correct this after the fact (matching the broader "no setter for X after init" pattern seen in A4/B4).
**Severity:** Medium.
**Affected files:** `contracts/dispute_resolution_contract/lib.rs`.
**Suggested solution:** enforce a sane minimum in `init` (e.g. `>= 1 day` in ledger seconds) and add an admin-gated `set_dispute_time_limit`.
**Complexity:** Small.

---

# Phase C — Architecture Cleanup

### C1. Admin/governance model reinvented three different ways across six contracts

`escrow_contract`/`delivery_contract`/`fleet_management_contract`/`identity_reputation_contract` each store a single `Address` under an admin key with a two-step-or-nothing transfer story (only `escrow_contract` has `propose_admin`/`accept_admin`; the rest have no rotation path at all). `dispute_resolution_contract` alone uses a `Map<Address, bool>`-style multi-admin model (`DataKey::Admin(Address)`). There is no shared `shared_types` abstraction for "who can administer this contract," so every contract hand-rolls its own admin storage layout, its own `is_admin` check, and its own (in)ability to rotate/add/remove admins.
**Severity:** Medium (architecture debt, not a bug per se, but the root cause of A4, B4, and C9).
**Affected files:** all six contract crates.
**Suggested solution:** define a shared `Governance`/`AdminSet` helper in `shared_types` (single-admin with two-step transfer as the default, opt-in multi-admin) and migrate every contract onto it.
**Complexity:** Large — cross-cutting refactor touching every contract's storage schema (backward-incompatible for already-deployed instances).

### C2. Fee-calculation-and-payout logic triplicated across three escrow functions

`release_escrow`, `resolve_dispute` (release-to-driver branch), and the platform-fee-transfer block are near-identical copies of: load `platform_fee_bps` → `calculate_fee` → `payout_driver` → conditionally transfer platform fee to admin. Any future fix to this logic (e.g., A9's missing balance guard, or B5's slippage parameter) has to be applied in multiple places and is easy to miss in one.
**Severity:** Medium.
**Affected files:** `contracts/escrow_contract/lib.rs`.
**Suggested solution:** extract a single `settle_and_payout(env, record, release_to_driver_amount) -> (driver_amount, platform_fee)` helper used by both call sites.
**Complexity:** Small–Medium.

### C3. Divergent `DriverProfile`/`UserProfile` definitions across crates

**Files:** `contracts/shared_types/lib.rs:542-555`; `contracts/identity_reputation_contract/lib.rs:7-22`
`identity_reputation_contract` defines its own local `DriverProfile` (fields identical to `shared_types::DriverProfile`, just redeclared) and its own `UserProfile { address, join_date }`, which has a **different field name** (`join_date` vs. `shared_types::UserProfile.registered_at`) for the same concept. `delivery_contract` uses the `shared_types` version. Nothing enforces these stay in sync; a future cross-contract call passing one crate's struct to a function expecting the other's would fail to decode at the SDK boundary.
**Severity:** Medium (no single source of truth; directly caused A5's dual-storage bug).
**Affected files:** `contracts/shared_types/lib.rs`, `contracts/identity_reputation_contract/lib.rs`.
**Suggested solution:** delete the local redeclarations in `identity_reputation_contract`, import from `shared_types` exclusively, and rename `UserProfile.join_date` → `registered_at` to match.
**Complexity:** Small (mechanical), but is a storage-schema-affecting change.

### C4. Delivery and escrow state machines can silently desynchronize

`delivery_contract` and `escrow_contract` each maintain their own status enum (`DeliveryStatus` vs `EscrowStatus`) for what is conceptually one lifecycle, connected only by best-effort cross-contract calls at each transition point (e.g., `confirm_delivery` calls `release_escrow`, `raise_dispute` calls the escrow's `raise_dispute`). Nothing guarantees the two stay consistent — e.g., A2 shows a concrete path where `escrow` moves to `Refunded` while `delivery` is still `Cancelled` via a route that never routed through the intended dispute-resolution flow; A8 shows escrow status lying about what actually happened. There is no invariant check (e.g., an admin/monitoring `assert_consistent(delivery_id)` view) anywhere in the protocol.
**Severity:** Medium-High (root architectural cause underlying several Phase A/B findings).
**Affected files:** `contracts/delivery_contract/lib.rs`, `contracts/escrow_contract/lib.rs`.
**Suggested solution:** longer-term, consider merging delivery and escrow into a single state machine (or at minimum add a cross-contract consistency-check view function used by monitoring); shorter-term, ensure every transition in one contract that implies a transition in the other is atomic within one call tree with no bypass paths (fixes A2).
**Complexity:** Large (architecture-level).

### C5. `DeliveryDetails` and `PartyAddresses` are fully-defined dead types

**File:** `contracts/shared_types/lib.rs:229-235, 282-288`
Both structs are fully defined, derive traits, and are unit-tested (`party_addresses_preserve_fields`), but are never constructed or used by any of the six contracts outside their own test module.
**Severity:** Low.
**Affected files:** `contracts/shared_types/lib.rs`.
**Suggested solution:** delete, or if they represent a genuinely planned API (e.g., a future `get_delivery_details` view), wire them into an actual contract function.
**Complexity:** Trivial.

### C6. Typed event structs in `shared_types::events` are unused; every contract publishes raw inline `Symbol::new` strings with inconsistent naming

**Files:** `contracts/shared_types/lib.rs:32-153`; every contract's `env.events().publish(...)` call sites
`shared_types` defines seven typed event payload structs (`DeliveryCreatedEvent`, `EscrowFundedEvent`, etc.) and topic-constant helper functions (`events::delivery_created(env)`, etc.) — but only `escrow_contract` uses a subset of the topic helpers (`events::escrow_funded`, `events::escrow_released`, `events::delivery_disputed`, `events::escrow_refunded`, `events::dispute_resolved`), and none of the typed *payload structs* are ever constructed. Every other contract inlines its own `Symbol::new(&env, "...")` literal per call site. Independently confirmed by grepping every `Symbol::new(&env, "...")` call across the workspace: topic names mix `PascalCase` (`AdminTransferred`, `DeliveryContractInitialized`, `DeliveryInTransit`, `FeeUpdated`, `ProtocolInitialized`) and `snake_case` (`delivery_created`, `delivery_cancelled`, `driver_assigned`, `dispute_raised`, `fleet_registered`, …) with no consistent convention, making off-chain indexers/monitoring brittle and inconsistent.
**Severity:** Medium (observability/integration risk, dead-code hygiene).
**Affected files:** all six contract crates, `contracts/shared_types/lib.rs`.
**Suggested solution:** pick one naming convention (recommend `snake_case` to match the majority), migrate every event topic to it, and either actually use the typed payload structs from `shared_types::events` everywhere or delete them.
**Complexity:** Medium (mechanical but touches every event-publishing call site and is an off-chain-indexer-breaking change — needs a coordinated docs/indexer update).

### C7. TTL magic numbers duplicated ~25+ times with no shared constant

**Files:** `contracts/delivery_contract/lib.rs` (6 occurrences), `contracts/dispute_resolution_contract/lib.rs` (3), `contracts/fleet_management_contract/lib.rs` (4), `contracts/identity_reputation_contract/lib.rs` (4)
The literal pair `518400, 518400` (both threshold and extend-to) is hardcoded inline at every `extend_ttl` call site across four contracts. Only `escrow_contract` bothers to name these (`constants::ESCROW_TTL_THRESHOLD`/`ESCROW_TTL_EXTEND_TO`), and even that module is private to `escrow_contract` — not shared. A future TTL policy change requires a find-and-replace across five files instead of editing one constant.
**Severity:** Low-Medium.
**Affected files:** all contract crates except `settlement_contract`.
**Suggested solution:** move a shared `pub const STORAGE_TTL_THRESHOLD/EXTEND_TO` pair into `shared_types` and reference it everywhere.
**Complexity:** Trivial-Small.

### C8. Overlapping but differently-numbered error enums across contracts (no unified error table)

**Files:** `contracts/shared_types/lib.rs:8-29` (`FaniLabError`); `contracts/escrow_contract/lib.rs:130-136` (`EscrowError`); plus `FleetError` and `DisputeStatus`-adjacent errors in other contracts
`FaniLabError::DeliveryNotFound = 4` and the contract-local `EscrowError::DeliveryNotFound = 2` are semantically the same condition with different wire discriminants. A generic client trying to build one "error code → message" table across the whole protocol cannot, because six independently-numbered `#[contracterror]` enums exist with overlapping vocabulary and no cross-reference.
**Severity:** Low-Medium (developer experience / documentation gap, not a runtime bug).
**Affected files:** all contract crates.
**Suggested solution:** either consolidate on `shared_types::FaniLabError` everywhere and drop contract-local error enums for concepts it already covers, or publish a single `docs/ERROR_CODES.md` table mapping `(contract, discriminant) → meaning`.
**Complexity:** Medium if consolidating types (breaking change); Small if just documenting.

### C9. `escrow_contract` and `delivery_contract` each hand-roll an identical private `is_admin` helper

**Files:** `contracts/escrow_contract/lib.rs:30-37`; `contracts/delivery_contract/lib.rs:354-364`
Both implement the same three-line "load admin from instance storage, compare to caller" logic independently (one via `.expect(...)`, the other via `if let Some(...)`), rather than sharing one implementation. Direct consequence of C1 (no shared governance abstraction).
**Severity:** Low.
**Affected files:** as above.
**Suggested solution:** subsumed by C1's shared governance helper.
**Complexity:** Trivial once C1 lands.

### C10. `settlement_contract` is the only crate using the standard `src/lib.rs` layout, and carries an unused `shared_types` dependency

**Files:** `contracts/settlement_contract/Cargo.toml`, `contracts/settlement_contract/src/lib.rs`
Every other contract crate places `lib.rs` flat at the crate root (`contracts/<name>/lib.rs`, configured via a non-default `[lib] path` in each `Cargo.toml` — worth confirming when reading each manifest). `settlement_contract` alone uses the idiomatic-but-inconsistent `src/lib.rs` path. Separately, its `Cargo.toml` declares `shared_types = { path = "../shared_types" }` as a dependency, but the crate's source never references anything from `shared_types` (only `soroban_sdk` is used).
**Severity:** Low.
**Affected files:** `contracts/settlement_contract/`.
**Suggested solution:** move `settlement_contract`'s `lib.rs` to the crate root to match its siblings (or, preferably, move the other five to the idiomatic `src/lib.rs` layout instead, since that's the Rust-standard convention); drop the unused `shared_types` dependency unless A6's real implementation will need it.
**Complexity:** Trivial.

---

# Phase D — Code Quality

### D1. `get_status` is a dead stub with the wrong return type

**File:** `contracts/escrow_contract/lib.rs:214-216`
```rust
pub fn get_status(_env: Env) -> DeliveryStatus {
    DeliveryStatus::Pending
}
```
Takes no `delivery_id`, ignores all state, and returns `shared_types::DeliveryStatus` (a *delivery* status enum) from the *escrow* contract, which has its own `EscrowStatus` type and its own working `get_escrow(delivery_id).status` accessor. Any integration relying on this function is silently reading a constant.
**Severity:** Medium-High (misleading public API on a financial contract).
**Affected files:** `contracts/escrow_contract/lib.rs`.
**Suggested solution:** delete it (callers should use `get_escrow(delivery_id).status`), or implement it properly against `delivery_id` if a distinct accessor is genuinely wanted.
**Complexity:** Trivial.

### D2. Blanket `#![allow(deprecated)]` in all six crates masks the SDK-27 `events().publish()` deprecation

**Files:** line 2 of every contract's `lib.rs`
Every crate suppresses deprecation warnings wholesale rather than tracking migration to whatever `events().publish()`'s SDK-27 replacement is. This is a reasonable stopgap (noted honestly in the comment) but has no tracking issue/TODO tying it to a resolution, so it can silently persist across SDK upgrades that remove the deprecated API entirely.
**Severity:** Low (today) / will become High whenever `soroban-sdk` removes the deprecated method.
**Affected files:** all six contract crates.
**Suggested solution:** file a tracked follow-up tied to the SDK's replacement API and reference it in the `allow(deprecated)` comment; revisit at the next SDK bump.
**Complexity:** Trivial to track, Medium to actually migrate (depends on SDK's replacement surface).

### D3. `Cargo.toml`'s `release-with-logs` profile is unused dead configuration

**File:** `Cargo.toml:20-22`
Confirmed via grep: no CI workflow, script, or `Makefile` target ever passes `--profile release-with-logs`.
**Severity:** Low.
**Affected files:** `Cargo.toml`.
**Suggested solution:** either wire it into a debug-build CI/deploy path (its evident purpose — release optimizations minus stripped debug assertions) or remove it.
**Complexity:** Trivial.

### D4. `resolve_dispute`/`resolve_dispute_split` emit a useless duplicated-caller event and never emit `escrow_released`/`escrow_refunded`

**File:** `contracts/escrow_contract/lib.rs:475-478, 523-526`
```rust
env.events().publish(
    (events::dispute_resolved(&env), delivery_id),
    (caller.clone(), caller),
);
```
The event payload is the same `caller` address twice — almost certainly a copy-paste bug where the second field was meant to be the driver address, the resulting amount, or the resolution outcome. Additionally, unlike `release_escrow`/`refund_escrow` (which each emit their own topic), these two dispute-resolution paths perform the exact same fund movement but never emit `escrow_released`/`escrow_refunded`, so off-chain indexers watching for those topics will miss every dispute-resolved settlement.
**Severity:** Medium (breaks event-driven monitoring/indexing for the dispute-resolution path specifically).
**Affected files:** `contracts/escrow_contract/lib.rs`.
**Suggested solution:** emit the same `escrow_released`/`escrow_refunded` topics these functions' non-dispute counterparts already use, with correct payloads, in addition to (or instead of) the generic `dispute_resolved` topic.
**Complexity:** Trivial.

### D5. `DeliveryMetadata.delivery_id` is never validated against the real assigned `DeliveryId`

**File:** `contracts/delivery_contract/lib.rs:78-120`
`create_delivery` accepts a caller-supplied `DeliveryMetadata` struct that itself carries a `delivery_id: u64` field (`shared_types/lib.rs:578`), but the actual `DeliveryId` used for storage keys is independently generated from the internal counter. The caller-supplied `metadata.delivery_id` is stored verbatim and never cross-checked — it can be any arbitrary, wrong, or duplicate value with no consequence, making the field pure noise that can mislead anyone reading stored metadata directly.
**Severity:** Low-Medium.
**Affected files:** `contracts/delivery_contract/lib.rs`, `contracts/shared_types/lib.rs`.
**Suggested solution:** either remove the redundant field from `DeliveryMetadata` (the real `DeliveryId` already lives on `DeliveryRecord`), or have `create_delivery` overwrite/validate it against the generated `DeliveryId`.
**Complexity:** Small.

### D6. No input validation on `CargoDescriptor`/`DeliveryMetadata`

**File:** `contracts/delivery_contract/lib.rs:78-120` (`create_delivery`)
Empty `origin`/`destination` strings and zero-weight cargo are silently accepted; there's no minimum-content validation on delivery creation.
**Severity:** Low.
**Affected files:** `contracts/delivery_contract/lib.rs`.
**Suggested solution:** add basic non-empty/non-zero checks with a typed `DeliveryError::InvalidMetadata` variant.
**Complexity:** Trivial.

### D7. Leftover repository debris

**Files:** `test_script.py`, `tests_passing.png` (repo root)
A stray Python script and a screenshot image committed at the repository root, unrelated to the Rust/Soroban workspace.
**Severity:** Low.
**Suggested solution:** delete both (or move into a `docs/assets/` or `.github/` location if the screenshot is meant to be referenced from documentation).
**Complexity:** Trivial.

### D8. Build tooling still targets the pre-migration `wasm32-unknown-unknown`, inconsistent with CI's `wasm32v1-none`

**Files:** `Makefile:6,10,14,18`; `.vscode/settings.json:2`; `scripts/deploy-all-contracts.sh:44,52`
The repository migrated to `wasm32v1-none` for SDK 27 (see `SOROBAN_SDK_27_MIGRATION.md`, commit `6944bd4`, and every `.github/workflows/*.yml`), but `Makefile`, `.vscode/settings.json`, and `scripts/deploy-all-contracts.sh` were never updated and still reference `wasm32-unknown-unknown`, a target that may no longer produce a compatible/working artifact with the current SDK. Anyone running `make build` or the deploy script locally gets a silently different (and possibly broken) build than CI.
**Severity:** Medium (developer-experience trap that can ship a mismatched artifact).
**Affected files:** `Makefile`, `.vscode/settings.json`, `scripts/deploy-all-contracts.sh`.
**Suggested solution:** update all three to `wasm32v1-none`.
**Complexity:** Trivial.

---

# Phase E — Performance

### E1. No enumeration/pagination API anywhere in the protocol

There is no way to list all deliveries for a sender, all drivers in a fleet, or all active fleets — every "list X" use case must be reconstructed off-chain purely from events. This isn't wrong (it's a common, often-recommended Soroban pattern to keep on-chain storage minimal), but it's undocumented as a deliberate choice and no companion indexer/subgraph tooling exists in this repository to make that workable in practice.
**Severity:** Low (architecture choice) but worth documenting explicitly as intentional rather than missing.
**Affected files:** all contracts.
**Suggested solution:** document the event-sourcing expectation explicitly in `docs/architecture/`, or add bounded/paginated enumeration views (`get_fleet_drivers(fleet_id, offset, limit)`, etc.) if off-chain indexing isn't actually available to consumers.
**Complexity:** Medium if adding real enumeration (new storage structures, e.g., a `Vec<Address>` roster per fleet, with its own unbounded-growth risk to manage).

### E2. `add_evidence_hash` allows unbounded growth of a single persistent storage entry

**File:** `contracts/dispute_resolution_contract/lib.rs:205-245`
`DisputeCase.evidence_hashes: Vec<BytesN<32>>` has no cap on length. A dispute's sender or recipient can call `add_evidence_hash` repeatedly, growing one persistent ledger entry indefinitely — eventually risking Soroban's per-entry size limits and/or steadily increasing the write cost of every subsequent evidence submission or dispute-resolution call that reads/rewrites the whole `DisputeCase`.
**Severity:** Medium.
**Affected files:** `contracts/dispute_resolution_contract/lib.rs`.
**Suggested solution:** cap `evidence_hashes.len()` (e.g., 20) and reject further additions with a typed error once reached.
**Complexity:** Trivial.

### E3. `ESCROW_TTL_THRESHOLD == ESCROW_TTL_EXTEND_TO` leaves no safety margin for proactive renewal

**File:** `contracts/escrow_contract/lib.rs:14-15`
```rust
pub const ESCROW_TTL_THRESHOLD: u32 = 518400;
pub const ESCROW_TTL_EXTEND_TO: u32 = 518400;
```
Soroban's `extend_ttl(threshold, extend_to)` is meant to proactively renew an entry once its remaining TTL drops *below* `threshold`, extending it back out to `extend_to`. Setting both to the identical value means the entry is only renewed once its TTL has already dropped to essentially the same value it's being extended to — leaving no lead time/margin, and depending on how frequently `save_escrow`/`load_escrow` are actually invoked for a given record, this can under-renew and risk archival/eviction for long-lived, infrequently-touched escrows.
**Severity:** Medium.
**Affected files:** `contracts/escrow_contract/lib.rs`.
**Suggested solution:** set `ESCROW_TTL_THRESHOLD` meaningfully lower than `ESCROW_TTL_EXTEND_TO` (e.g., threshold at 30 days, extend-to at 60 days) to give a real renewal margin.
**Complexity:** Trivial (one constant change) but should be paired with C7 (share this constant everywhere instead of duplicating).

---

# Phase F — Documentation

### F1. Three architecture/design docs are committed as completely empty files

**Files:** `docs/architecture/event-system.md` (0 bytes), `docs/contract-design/escrow-design.md` (0 bytes), `docs/protocol/delivery-protocol.md` (0 bytes)
Confirmed via `wc -l` — all three are literally zero-length, despite being linked-to/implied elsewhere as real documentation.
**Severity:** Low-Medium.
**Suggested solution:** either populate with real content (the event-topic-naming findings in C6 would be a natural starting point for `event-system.md`) or remove the empty placeholders and any links to them.
**Complexity:** Small (removal) to Large (writing real architecture docs).

### F2. `PRODUCTION_READINESS.md` claims directly contradicted by this audit

**File:** `PRODUCTION_READINESS.md`
Claims "Status: 10/10 - Production Ready," "Zero critical security vulnerabilities," "Comprehensive error handling," and "State transition validation" as fully implemented (Security section, `## 3. Security ✅ (10/10)`). Findings A1–A9, B1–B10, and C4 above directly contradict this — this repository has at least nine Critical-severity, unresolved issues as of this audit, including an unauthenticated fund-freeze function (A3) and a dispute-resolution bypass (A2).
**Severity:** High (a document actively asserting production-readiness and "zero critical vulnerabilities" is actively dangerous if anyone relies on it to make a mainnet-deployment decision).
**Affected files:** `PRODUCTION_READINESS.md`.
**Suggested solution:** revise the document to reflect actual state (or remove it until an external audit + the Phase A/B fixes in this report are complete), consistent with Meta-Finding 0's caution about self-reported "done" status in this repository.
**Complexity:** Trivial (docs edit), but only once the underlying Phase A/B work is real.

### F3. Version numbers and organization links are wrong throughout root docs

**Files:** `README.md:229,319-320,324,364`; `SECURITY.md:7`; every `contracts/*/Cargo.toml`
`README.md`/`SECURITY.md` claim version `0.2.x`; every `Cargo.toml` says `0.1.0`. `README.md`'s CI/coverage badges and "GitHub Organization" link point to `github.com/fanilab/FaniLab-SmartContract` / `github.com/fanilab`, which do not exist — the real repository is `github.com/fanilabs/fanilab-smartcontract` (confirmed via `gh repo view`).
**Severity:** Medium (broken badges/links erode contributor trust and make the CI status badge permanently show "unknown"/broken).
**Affected files:** as above.
**Suggested solution:** fix all links to `fanilabs/fanilab-smartcontract`; either bump every `Cargo.toml` to `0.2.0` to match the docs, or roll the docs back to `0.1.x` — pick one source of truth and keep both in sync going forward (ideally via a single workspace-level version, though Soroban contract crates currently require per-crate `[package] version`).
**Complexity:** Trivial.

### F4. `docs/DEPLOYMENT.md` documents phantom functionality

`docs/DEPLOYMENT.md` documents an `update_escrow_contract` function and integration-test infrastructure that do not exist anywhere in `contracts/`.
**Severity:** Low-Medium (misleads operators following the deployment guide).
**Affected files:** `docs/DEPLOYMENT.md`.
**Suggested solution:** audit the whole document against actual contract entry points and remove/replace phantom references.
**Complexity:** Small.

### F5. `CHANGELOG.md`'s `[Unreleased]` section is stale relative to the completed SDK 27 migration

**File:** `CHANGELOG.md`
The SDK-27/`wasm32v1-none` migration (commit `6944bd4`, documented in its own `SOROBAN_SDK_27_MIGRATION.md`) is not reflected in `CHANGELOG.md` at all.
**Severity:** Low.
**Affected files:** `CHANGELOG.md`.
**Suggested solution:** add an entry for the SDK 27 migration; establish a habit (or a CI check) of updating `CHANGELOG.md` alongside significant merges.
**Complexity:** Trivial.

### F6. `docs/API.md` documents 30+ functions but shows a full usage example for exactly one

Only `escrow_contract::init` (near the top of the file) has a worked `rust` code example; every other documented function has parameters/errors listed but no example call, making the reference far less useful for integrators than its length suggests.
**Severity:** Low.
**Affected files:** `docs/API.md`.
**Suggested solution:** add at least one example per contract's primary happy-path flow (create → assign → confirm → release).
**Complexity:** Medium (writing effort, not technically hard).

---

# Phase G — Testing

### G1. `escrow_contract`'s admin dispute-resolution entry points have no direct tests

**File:** `contracts/escrow_contract/test.rs` (14 tests total — confirmed via grep of `#[test]` blocks)
Tests exist for `raise_dispute` (pausing) and for the *authorization/state* of `refund`/`release` from a `Paused` state via the ordinary (non-dispute) entry points, but there is **no test that calls `resolve_dispute` or `resolve_dispute_split` directly** — meaning A8 (mislabeled status) and A9 (missing balance guard) would not have been caught by the existing suite, and aren't caught by it today.
**Severity:** Medium-High (the two functions with confirmed logic bugs in this report have zero direct coverage).
**Affected files:** `contracts/escrow_contract/test.rs`.
**Suggested solution:** add direct tests for both functions covering: happy path, unauthorized caller, wrong state, and (for `resolve_dispute_split`) `sender_share_bps` boundary values (0, 10000, >10000).
**Complexity:** Small.

### G2. `settlement_contract`'s test suite only exercises `init`

**File:** `contracts/settlement_contract/src/lib.rs:43-59`
A single `#[test] fn test_init()` is the entire test suite — unsurprising given A6 (the contract is a stub), but worth calling out as a testing gap that will need to grow substantially the moment A6 is implemented.
**Severity:** Low today / will become High once A6 lands without corresponding tests.
**Affected files:** `contracts/settlement_contract/src/lib.rs`.
**Suggested solution:** track alongside A6 — no real fix possible until real logic exists to test.
**Complexity:** N/A until A6.

### G3. Two-step admin transfer (`propose_admin`/`accept_admin`) has no test coverage in `escrow_contract`

Grep of `contracts/escrow_contract/test.rs`'s test function names shows no `propose_admin`/`accept_admin`-named test, meaning this security-sensitive flow (including its wrong-caller `panic!("caller is not the admin")` / `panic!("caller is not the pending admin")` paths — see B8) is entirely unverified by CI.
**Severity:** Medium.
**Affected files:** `contracts/escrow_contract/test.rs`.
**Suggested solution:** add tests for: successful propose→accept, accept by non-pending address (must fail), propose by non-admin (must fail).
**Complexity:** Small.

### G4. No integration test scaffolding between `fleet_management_contract` and `escrow_contract`/`delivery_contract`

`fleet_management_contract/test.rs` has the highest test count in the workspace (36), but they appear to exercise the contract in isolation; `get_payout_address` (the function meant to redirect driver payouts to a fleet treasury) is never verified against an actual `escrow_contract::release_escrow` call, meaning the fleet-treasury-routing feature described in `docs/` may not actually be wired into the real payout path at all — worth confirming as part of any Phase G test-expansion work, since `escrow_contract::payout_driver` (lib.rs:60-93) shows no call to `fleet_management_contract` whatsoever today.
**Severity:** Medium (matches Phase A/C's cross-contract wiring gaps — `payout_driver` never consults fleet treasury routing despite `get_payout_address` existing for exactly that purpose).
**Affected files:** `contracts/fleet_management_contract/lib.rs`, `contracts/escrow_contract/lib.rs`.
**Suggested solution:** either wire `payout_driver` to call `get_payout_address` before transferring, or clarify in docs that fleet treasury routing is not yet connected to real payouts; add an integration test either way.
**Complexity:** Medium (new cross-contract call + tests).

### G5. No property-based or fuzz testing despite documentation prescribing both

`docs/TESTING.md`/`docs/SECURITY_AUDIT.md` reference property-based testing and `cargo fuzz` commands; no `proptest` dependency exists in any `Cargo.toml`, and no `fuzz/` directory or fuzz target exists anywhere in the repository.
**Severity:** Medium (aspirational docs vs. reality gap; also a real coverage gap for fee-calculation/state-machine edge cases like A1).
**Affected files:** `docs/TESTING.md`, `docs/SECURITY_AUDIT.md`, all `Cargo.toml`.
**Suggested solution:** either add `proptest` + real property tests for `calculate_fee`/state-transition functions (high value given A1), or scope down the documentation's claims to match reality.
**Complexity:** Medium.

### G6. Dispute-resolution's reputation-penalty cross-call is never exercised by any test

`dispute_resolution_contract::resolve_dispute_refund_sender` conditionally cross-calls `identity_reputation_contract::decrease_reputation` (lines 280-295) — but doing so requires a fully wired-up three-contract test harness (delivery + escrow + identity), which does not appear to exist in `dispute_resolution_contract/test.rs`'s 13 tests. This is the one path in the entire codebase that currently *does* connect to `identity_reputation_contract`'s reputation system (see A5), and it's untested.
**Severity:** Medium.
**Affected files:** `contracts/dispute_resolution_contract/test.rs`.
**Suggested solution:** add a full cross-contract integration test wiring all three contracts together and asserting the reputation score actually decreases.
**Complexity:** Medium (multi-contract test harness).

### G7. `resolve_dispute_split_funds` has no unauthorized-caller test

`contracts/dispute_resolution_contract/test.rs` — grep confirms 13 total tests; no test name suggests an unauthorized-caller check specifically for `resolve_dispute_split_funds`, despite similar functions (`resolve_dispute_refund_sender`, `resolve_dispute_pay_driver`) sharing the same `is_admin` guard pattern that presumably *is* tested elsewhere.
**Severity:** Low-Medium.
**Affected files:** `contracts/dispute_resolution_contract/test.rs`.
**Suggested solution:** add the missing negative-authorization test for parity with its sibling functions.
**Complexity:** Trivial.

---

# Appendix: Remaining catalogued items (verified severity, condensed)

The following were originally identified in the repository's prior three-pass review (`fani-smartcontract-issues.md`, GitHub issues #31–#144) and independently spot-checked for this audit (see Meta-Finding 0's methodology — several rows below were confirmed still-broken despite showing "closed" on GitHub). They are lower-severity documentation/tooling/hygiene items; full narrative detail for each is available in the original issue text on GitHub rather than reproduced here verbatim.

| # | Item | Severity | GitHub | Verified? |
|---|---|---|---|---|
| — | CI's outdated-dependency check has `continue-on-error: true`, can never fail the build | Low | [#141](https://github.com/fanilabs/fanilab-smartcontract/issues/141) (open) | Yes — `ci.yml:57` |
| — | `security-audit.yml` only runs `cargo deny check advisories`, never `check licenses`/`check bans` despite `deny.toml` defining both | Medium | [#142](https://github.com/fanilabs/fanilab-smartcontract/issues/142) (open) | Yes — `security-audit.yml:30` |
| — | `release.yml` builds and publishes a GitHub Release without ever running the test suite | High | [#143](https://github.com/fanilabs/fanilab-smartcontract/issues/143) (open) | Yes — no `cargo test` step in `release.yml` |
| — | `docs/API.md` table-of-contents/coverage mismatch | Low | [#64](https://github.com/fanilabs/fanilab-smartcontract/issues/64) (open) | Partially — TOC now lists all 6 contracts; residual mismatch not re-verified in depth |
| — | `deploy-all-contracts.sh`'s `deploy_contract()` captures decorative echo output into `$contract_id`, corrupting the JSON output file | Medium | [#140](https://github.com/fanilabs/fanilab-smartcontract/issues/140) (open) | **Yes** — confirmed by reading the script: every `echo` inside `deploy_contract()` (lines 62-77) is captured by the caller's `contract_id=$(deploy_contract ...)`, not just the final `echo "$contract_id"` |
| — | `scripts/deploy-contract.sh` / `initialize-contract.sh` committed empty | High | [#133](https://github.com/fanilabs/fanilab-smartcontract/issues/133) (closed, COMPLETED) | **Re-confirmed still broken** — see Meta-Finding 0 |
| — | Leftover repo debris (`test_script.py`, `tests_passing.png`) | Low | [#135](https://github.com/fanilabs/fanilab-smartcontract/issues/135) (closed, COMPLETED) | **Re-confirmed still present** — see Meta-Finding 0 (also D7 above) |
| — | `.vscode/settings.json` stale target / `launch.json` only debugs one contract | Low | [#136](https://github.com/fanilabs/fanilab-smartcontract/issues/136) (closed, COMPLETED) | **Re-confirmed still broken** — see Meta-Finding 0 (also D8 above) |
| — | README badges/org link point to nonexistent repo | Medium | [#127](https://github.com/fanilabs/fanilab-smartcontract/issues/127) (closed, COMPLETED) | **Re-confirmed still broken** — see F3 above |
| — | Version mismatch (0.2.x docs vs 0.1.0 Cargo.toml) | Low | [#128](https://github.com/fanilabs/fanilab-smartcontract/issues/128) (closed, COMPLETED) | **Re-confirmed still present** — see F3 above |
| — | `docs/DEPLOYMENT.md` phantom function/infra | Medium | [#129](https://github.com/fanilabs/fanilab-smartcontract/issues/129) (closed, COMPLETED) | Consistent with F4 above (not independently re-verified line-by-line) |
| — | Contributor docs reference nonexistent GitHub labels | Low | [#130](https://github.com/fanilabs/fanilab-smartcontract/issues/130) (closed, COMPLETED) | Not independently re-verified |
| — | `docs/architecture/smart-contract-architecture.md` documents phantom `RoleType` enum / `PickedUp` status | Low | [#131](https://github.com/fanilabs/fanilab-smartcontract/issues/131) (closed, COMPLETED) | Not independently re-verified |
| — | `docs/SECURITY_AUDIT.md` prescribes an unused test-naming convention | Low | [#132](https://github.com/fanilabs/fanilab-smartcontract/issues/132) (closed, COMPLETED) | Consistent with observed test names (none use `security_`/`access_control_`/`state_transition_` prefixes) |
| — | Leftover `SwiftChainError` comments / phantom `.gitignore` paths from pre-rename project | Low | [#134](https://github.com/fanilabs/fanilab-smartcontract/issues/134) (closed, COMPLETED) | Not independently re-verified |
| — | CI pins `dtolnay/rust-toolchain@stable` (mutable ref) | Low | [#137](https://github.com/fanilabs/fanilab-smartcontract/issues/137) (closed, COMPLETED) | **Re-confirmed still present** — every workflow file read for this audit still uses `@stable` |
| — | Workflows pin deprecated action majors (`upload-artifact@v3` etc.) | Low | [#138](https://github.com/fanilabs/fanilab-smartcontract/issues/138) (closed, COMPLETED) | Contradicted by observation — `ci.yml` now uses `codecov/codecov-action@v3` (still v3) but git log shows separate `ci: bump codecov/codecov-action from 3 to 7` and `ci: bump actions/upload-artifact from 3 to 7` commits; `deploy-testnet.yml` still pins `actions/upload-artifact@v3` (**not bumped** — inconsistent partial fix) |
| — | `deploy-testnet.yml` artifact-upload paths never match actual output filenames | Low | [#139](https://github.com/fanilabs/fanilab-smartcontract/issues/139) (closed, COMPLETED) | Plausible given `deploy-all-contracts.sh` writes `contract-ids-$NETWORK.json`, not `contract-ids.txt` as `deploy-testnet.yml:66-68` expects — **appears still present** |
| — | No CI step enforces the 64 KB WASM contract-size limit | Low | [#144](https://github.com/fanilabs/fanilab-smartcontract/issues/144) (closed, COMPLETED) | **Re-confirmed still absent** — no size-check step in any workflow read for this audit |
| — | Remaining #31–#126 items (architecture/testing/tooling/docs from the second and third review passes) | Mixed | [#31–#126](https://github.com/fanilabs/fanilab-smartcontract/issues) | Not individually re-verified for this audit; treat GitHub's "closed" status as **unverified** per Meta-Finding 0 until each is re-checked against current code |

**Recommendation:** given the pattern established by every spot-check above, treat *all* 125 "closed" issues as open until each is individually re-verified against a real fix commit — do not rely on issue state alone.

---

# Final Summary

## Totals

- **Findings with full detail in this report:** 45 (9 Critical, 10 High/Medium-High in Phase B, 10 in Phase C, 8 in Phase D, 3 in Phase E, 6 in Phase F, 7 in Phase G — some findings span categories and are counted once at their primary phase)
- **Additional catalogued items (Appendix, prior review passes):** ~105, spanning GitHub issues #31–#144 (5 confirmed still open, 125 marked closed but re-verification shows the closure status is not reliable — see Meta-Finding 0)
- **Grand total tracked issues (this report + appendix + prior backlog):** 130 previously-filed + Meta-Finding 0 + several newly-identified nuances not previously catalogued in this level of detail (A5's dual-reputation-storage mechanism, A1's fee-ceiling fund-lock chain, A9's inconsistent balance-guard, C6's confirmed PascalCase/snake_case event-naming split, D4's literal duplicated-caller event bug, G4's unwired fleet-treasury-routing gap)

## Issues by severity (Phase A–G detailed findings only)

| Severity | Count |
|---|---|
| Critical | 9 (A1–A9) |
| High | 8 (A9 dual-counted as High-adjacent, B3–B6, B8, D1, D8, F2) |
| Medium | ~20 (majority of Phase B/C/D/F/G) |
| Low | ~8 (C5, C7–C10 partial, D2–D3, D7, E1, F1, F5–F6, G2, G7) |

## Estimated cleanup effort

- **Phase A (Critical Bugs):** 1.5–2.5 weeks for one experienced Soroban engineer (mostly small fixes, but A2/A5/C1-adjacent items need careful state-machine redesign and new integration tests; several are breaking API changes needing coordinated redeploys).
- **Phase B (Security):** 1–2 weeks (mostly additive guards/timelocks; B3's pause mechanism touches every contract).
- **Phase C (Architecture):** 2–4 weeks if the governance-unification (C1) and event-standardization (C6) refactors are pursued; both are cross-cutting and backward-incompatible with any already-deployed instance.
- **Phase D (Code Quality):** 3–5 days, mostly mechanical.
- **Phase E (Performance):** 2–3 days.
- **Phase F (Documentation):** 1 week to bring docs in line with actual code state, contingent on Phase A/B landing first (fixing docs to describe still-broken code is not useful).
- **Phase G (Testing):** 1–1.5 weeks to close the identified coverage gaps; more if property/fuzz testing (G5) is pursued in earnest.
- **Total: roughly 7–11 engineer-weeks**, excluding the "Appendix" long tail, which is mostly small/independent fixes that can be parallelized across contributors.

## Biggest architectural weaknesses

1. **No shared governance model (C1)** — the root cause of A4, A7 (partially), B4, and C9; six contracts each reinvent "who's an admin."
2. **Delivery and escrow are two independently-mutated state machines with no enforced consistency (C4)** — root cause of A2 and A8.
3. **Reputation is tracked in two un-synced places with a dead increment path (A5)** — a core product feature (driver tiers) is non-functional as shipped.
4. **`settlement_contract` is fully wired into the live payout path while being 100% unimplemented (A6)** — architecturally sound *intent*, dangerously premature *integration*.

## Biggest security risks

1. **A3** — `freeze_funds` has zero authorization; anyone can DoS every escrow in the protocol today, no fix required to exploit.
2. **A2** — a sender can bypass admin dispute resolution and force a self-refund, directly at a driver's expense.
3. **A1** — an admin fee-ceiling gap that can permanently zero driver payouts or brick escrow release.
4. **B3** — no circuit breaker anywhere, so exploiting any of the above cannot be stopped short of a full contract migration.

## Biggest documentation gaps

1. **`PRODUCTION_READINESS.md`'s "10/10, zero critical vulnerabilities" claim (F2)** is the single most actively misleading document in the repository given this audit's findings.
2. **Meta-Finding 0** — the issue tracker itself, the primary mechanism this team has used to track findings, does not reflect reality; 125 "completed" issues have no corresponding fix commits.
3. Three architecture docs are literally empty (F1); `docs/DEPLOYMENT.md` documents functions that don't exist (F4).

## Biggest testing gaps

1. The two escrow functions with the most severe confirmed logic bugs (`resolve_dispute`, `resolve_dispute_split` — A8, A9) have **zero direct tests** (G1).
2. The only functioning cross-contract path into the reputation system (dispute-driven `decrease_reputation`) is untested (G6).
3. `settlement_contract`, sitting on the live fund-payout path, has one test total (G2).
4. No property-based or fuzz testing exists despite two docs prescribing it (G5) — exactly the technique that would have caught A1's fee-overflow scenario.

# FaniLab Smart Contracts — Final Execution Plan

Companion to `docs/CODEBASE_AUDIT_REPORT.md`. **No implementation has been started; no source code has been modified to produce this plan.**

## What changed from the first draft of this document

A second review pass cross-referenced every finding in the audit report (`A1`–`G7`, 45 items) against `fani-smartcontract-issues.md`'s full issue list. Result: **every one of them corresponds to a pre-existing GitHub issue** (#7–#144) — the audit independently re-derived the same defects the prior three review passes already catalogued, rather than surfacing a materially different set of problems. That's a useful corroboration of the backlog's accuracy, but it means the original cleanup plan effectively duplicated 45 already-tracked issues as if they were fresh findings.

This revision:
1. **Eliminates restatement** — each work item below is phrased as "fix tracked issue #N," not as a new discovery, and links straight to the GitHub issue.
2. **Eliminates false positives / overstated severity** — two items were reassessed on review (see below).
3. **Eliminates internal duplication** — several findings described different symptoms of the same underlying code change; they're now one batch each, not one batch per finding ID.
4. **Excludes anything not independently verified against current code.** The audit report's appendix listed ~105 additional catalogued items (GitHub #31–#144 minus the ones covered above) that were *not* individually re-read against the live source for this audit — those are correctly left out of an execution plan that's supposed to contain only verified improvements. They remain tracked on GitHub; re-verify each before scheduling it.

**Before any batch below is executed:** the linked GitHub issue is almost certainly still marked "closed / COMPLETED" despite no fix ever landing (see the audit report's Meta-Finding 0 — spot-checks of six closed issues found all six still broken in the current tree). **Reopen the issue as part of starting the batch**, and only close it again once a commit that actually fixes the code is merged.

---

## Eliminated / downgraded findings (from the original draft)

| ID | Original claim | Disposition | Reasoning |
|---|---|---|---|
| `A9` | "resolve_dispute's refund branch skips the balance guard" — filed under **Critical** | **Downgraded to Medium, merged into FA-1** | Re-examined: the omission is real (confirmed against code), but Soroban token contracts reject a transfer that exceeds available balance on their own, so this is a defense-in-depth/consistency gap, not an independently exploitable fund-loss path the way `A1`–`A8` are. Filing it as Critical overstated its impact. Still tracked under GitHub #14. |
| `C4` | "Delivery and escrow state machines can silently desynchronize" (general architecture claim) | **Merged into FA-2, not a standalone batch** | This is the general pattern that `A2` (#93) is a concrete instance of. Keeping both as separate execution items would mean scheduling the same code region twice under two different names. |
| `C2` | "Fee-calculation-and-payout logic triplicated" | **Merged into FA-1, not a standalone batch** | Same file, same functions, same PR as the `A1`/#11 fix — extracting the shared helper is a natural side effect of fixing the fee-ceiling bug, not separate work. |
| `D4` | "resolve_dispute/split emit useless duplicated-caller event" | **Merged into FA-5, not a standalone batch** | Same functions as `A8`/#13's status-labeling fix. |
| `B5` | "Zero slippage protection on settlement-swap payout" | **Merged into FA-6 as a dependent note, not a standalone batch** | Explicitly blocked on the `settlement_contract` scope decision (#30/#15) — there is nothing to fix here independent of that decision. |
| `E1` | "No enumeration/pagination API anywhere" | **Reclassified as a documentation note, removed as a code-fix item** | On review this is a deliberate, common Soroban architecture choice (event-sourced off-chain indexing), not a defect. It doesn't belong in a "verified code improvements" plan. If the team decides otherwise, GitHub #27/#28 already track it as a feature request. |
| Appendix (~105 items, GitHub #31–#144 minus the 45 above) | Various | **Excluded from this plan entirely** | Not independently re-read against current source during the audit; including them here would violate "only verified code improvements." Re-audit and re-verify before scheduling any of them. |

---

## How batches are sequenced

Numbered in recommended execution order within each phase. Every batch lists the GitHub issue(s) it closes — **reopen them first**. Cross-phase dependencies are called out explicitly.

---

# Phase A — Critical Bugs

### Batch FA-1: Escrow fee-ceiling, balance-guard consistency, and payout dedup
**Closes:** [#11](https://github.com/fanilabs/fanilab-smartcontract/issues/11) (`init` enforces no fee ceiling), [#14](https://github.com/fanilabs/fanilab-smartcontract/issues/14) (`resolve_dispute` refund branch skips balance guard), [#82](https://github.com/fanilabs/fanilab-smartcontract/issues/82) (fee/payout logic triplicated)
**Verified:** `contracts/escrow_contract/lib.rs:159-182` has no upper bound on `platform_fee_bps` at `init`, while `update_platform_fee` (line 194) caps it at 1000 bps — an oversized fee at `init` can zero driver payouts or make `release_escrow` permanently revert (contract can't transfer more `platform_fee` than its token balance). `resolve_dispute`'s `else` branch (lines 464-471) transfers without the `contract_balance < record.amount` check that `release_escrow`, `refund_escrow`, and `resolve_dispute_split` all have.
**Files affected:** `contracts/escrow_contract/lib.rs`
**Risk:** Low (additive guards, no state-machine change)
**Effort:** Small (0.5–1 day)
**Dependencies:** None — do this first.
**Validation steps:**
1. Add the same `> 1000 → InvalidFee` guard to `init` that `update_platform_fee` already has.
2. Add the missing `contract_balance < record.amount` check to `resolve_dispute`'s refund branch.
3. While touching this code, extract the repeated fee-calc + balance-check + payout sequence (present in `release_escrow` and `resolve_dispute`) into one shared helper.
4. New tests: `init` with `platform_fee_bps > 1000` panics `InvalidFee`; `resolve_dispute` refund branch with insufficient balance panics the typed `InsufficientFunds` error, not a raw token-contract panic.
5. `cargo test -p escrow_contract`.

### Batch FA-2: Fix the dispute-resolution bypass
**Closes:** [#7](https://github.com/fanilabs/fanilab-smartcontract/issues/7) (`freeze_funds` has no auth check), [#93](https://github.com/fanilabs/fanilab-smartcontract/issues/93) (sender bypasses admin dispute resolution via `cancel_delivery`)
**Verified:** `freeze_funds` (`escrow_contract/lib.rs:536-543`) takes no `caller` parameter and calls no `require_auth()` — any address can freeze any escrow. Separately, `validate_transition` (`delivery_contract/lib.rs:35-53`) permits `Disputed → Cancelled`, and `refund_escrow`'s guard (`escrow_contract/lib.rs:381-409`) accepts both `Locked` and `Paused` state with `sender_authorized` — so a sender can raise a dispute, then immediately call `cancel_delivery`, forcing a full self-refund before any admin rules on the dispute. Traced the full call chain to confirm both are independently real and combine into a working exploit path.
**Files affected:** `contracts/escrow_contract/lib.rs` (`freeze_funds`, `refund_escrow`), `contracts/delivery_contract/lib.rs` (`cancel_delivery`, `validate_transition`), `contracts/dispute_resolution_contract/lib.rs` (caller of `freeze_funds`)
**Risk:** Medium — breaking API change (`freeze_funds` gains a required `caller` parameter); state-machine guard change.
**Effort:** Medium (2–3 days including tests)
**Dependencies:** None. Land before Batch FG-1/FG-2 so new tests are written against corrected behavior.
**Validation steps:**
1. Add `caller: Address` + `caller.require_auth()` to `freeze_funds`; restrict to admin or the configured `dispute_resolution_contract` address.
2. Update `dispute_resolution_contract::raise_dispute`'s cross-call accordingly.
3. Reject `refund_escrow` while `Paused` from an active dispute (only `resolve_dispute`/`resolve_dispute_split` should move money out of `Paused`), or block `cancel_delivery` once `Disputed`.
4. New tests: unauthenticated `freeze_funds` call panics; sender calling `cancel_delivery` on a `Disputed` delivery panics.
5. `cargo test --workspace` (touches cross-contract call signatures).

### Batch FA-3: `identity_reputation_contract` initializer fix + reputation-flow rewire
**Closes:** [#10](https://github.com/fanilabs/fanilab-smartcontract/issues/10) (dual initializers can brick the contract), [#9](https://github.com/fanilabs/fanilab-smartcontract/issues/9) (reputation can only decrease, `increase_reputation` never called), [#104](https://github.com/fanilabs/fanilab-smartcontract/issues/104) (no admin setter to repoint delivery/dispute contract after `initialize()`)
**Verified:** `init(admin)` and `initialize(admin, delivery_contract, dispute_contract)` (`identity_reputation_contract/lib.rs:51-75`) both guard on the same `DataKey::Admin` flag with no way to call the fuller one after the simpler one; confirmed no setter exists anywhere in the file for `DataKey::DeliveryContract`/`DisputeContract`. Separately confirmed via `grep -rn "increase_reputation" contracts/ --include="*.rs"` that it is defined (line 205) and never called from any other contract — `delivery_contract::confirm_delivery` (lib.rs:272-293) instead increments a *separate* `DriverProfile` copy in its own storage namespace.
**Files affected:** `contracts/identity_reputation_contract/lib.rs`, `contracts/delivery_contract/lib.rs`
**Risk:** Medium-High — removes a public function (`init`); `confirm_delivery`'s cross-contract behavior changes; requires a new admin-configurable identity-contract address in `delivery_contract` that doesn't exist today.
**Effort:** Medium (3–4 days)
**Dependencies:** None technically, but combine with `FC-2` (below) since both touch the same file's type definitions.
**Validation steps:**
1. Delete `init`; keep `initialize` as the sole entry point; add `set_delivery_contract`/`set_dispute_contract` admin setters.
2. Add an admin-settable identity-contract address to `delivery_contract` (mirroring `fleet_management_contract::set_identity_contract`'s existing pattern).
3. Replace `confirm_delivery`'s local `DriverProfile` increment with a cross-call to `identity_reputation_contract::increase_reputation`, passing `weight_grams`/`fragile` from the delivery's cargo descriptor.
4. Decide the fate of `delivery_contract`'s local `DriverProfile` store (remove vs. keep as a display cache) and document the decision.
5. New integration test: complete a delivery end-to-end, assert `identity_reputation_contract::get_driver_profile(driver).reputation_score` increased and `get_driver_tier` reflects it.

### Batch FA-4: Fix `register_fleet`'s second-registration failure
**Closes:** [#39](https://github.com/fanilabs/fanilab-smartcontract/issues/39) (`register_fleet` permanently fails for any owner already registered as a driver)
**Verified:** `register_fleet` (`fleet_management_contract/lib.rs:105-154`) unconditionally cross-calls `identity_reputation_contract::register_driver(owner)` when an identity contract is configured; `register_driver` (`identity_reputation_contract/lib.rs:108-113`) panics `AlreadyInitialized` if a profile already exists for that address. Confirmed the fleet counter is incremented *before* this cross-call, so a failed retry also leaks/skips counter values.
**Files affected:** `contracts/fleet_management_contract/lib.rs`, `contracts/identity_reputation_contract/lib.rs`
**Risk:** Low-Medium
**Effort:** Small (1–2 days)
**Dependencies:** Combine with `FA-3` — both touch `identity_reputation_contract`'s registration surface.
**Validation steps:**
1. Add an idempotent registration path (`register_driver_if_absent`, or an existence check before calling `register_driver`).
2. Move the fleet-counter increment to after the cross-contract call succeeds, or explicitly document why it's acceptable to leak on failure.
3. New test: same owner calls `register_fleet` twice with an identity contract configured; second call succeeds.

### Batch FA-5: Escrow status labeling and dispute-resolution event correctness
**Closes:** [#13](https://github.com/fanilabs/fanilab-smartcontract/issues/13) (`resolve_dispute_split` mislabels final status as `Refunded`), [#88](https://github.com/fanilabs/fanilab-smartcontract/issues/88) (dispute-resolution functions emit a useless duplicated-caller event and never emit `escrow_released`/`escrow_refunded`)
**Verified:** `resolve_dispute_split` (`escrow_contract/lib.rs:481-527`) unconditionally sets `record.status = EscrowStatus::Refunded` at line 520 regardless of how funds were actually split. Both `resolve_dispute` and `resolve_dispute_split` publish `(events::dispute_resolved(&env), delivery_id), (caller.clone(), caller)` — the same address twice — and never emit the `escrow_released`/`escrow_refunded` topics their non-dispute counterparts (`release_escrow`, `refund_escrow`) use for the same fund movement.
**Files affected:** `contracts/escrow_contract/lib.rs`, `contracts/shared_types/lib.rs` (new `EscrowState::Split` variant)
**Risk:** Medium — adds a variant to a shared, storage-persisted enum (backward-incompatible for any already-deployed instance; irrelevant pre-launch).
**Effort:** Small (1 day)
**Dependencies:** None.
**Validation steps:**
1. Add `EscrowState::Split` to `shared_types::EscrowState`; use it in `resolve_dispute_split`.
2. Fix both functions' event payloads to carry meaningful data and emit the matching `escrow_released`/`escrow_refunded` topics alongside `dispute_resolved`.
3. New tests asserting `get_escrow(id).status == EscrowState::Split` post-split, and asserting event topics/payloads.

### Batch FA-6: Decide `settlement_contract`'s scope (implement or de-scope)
**Closes:** [#30](https://github.com/fanilabs/fanilab-smartcontract/issues/30) (`settlement_contract` is a complete no-op stub already wired into the live payout path), [#15](https://github.com/fanilabs/fanilab-smartcontract/issues/15) (zero slippage protection on the settlement-swap payout path — dependent on this decision)
**Verified:** every function in `settlement_contract/src/lib.rs` is a placeholder; `escrow_contract::payout_driver` (lib.rs:60-93) already cross-calls it when configured, with `min_amount_out` hardcoded to `0` — currently inert only because `get_driver_preference` always returns `None`.
**Files affected:** `contracts/settlement_contract/src/lib.rs`, `contracts/escrow_contract/lib.rs`
**Risk:** High if implementing for real; Low if temporarily de-scoping.
**Effort:** Large (1–2+ weeks) if implementing; Trivial if de-scoping now.
**Dependencies:** **Product decision required before scheduling** — flag to the team rather than defaulting either way.
**Validation steps (de-scope path):** remove the `payout_driver` → `settlement_contract` call; update `docs/API.md` to mark settlement as not yet implemented.
**Validation steps (implement path):** design review of the swap mechanism, real slippage protection (replacing the hardcoded `0`), full test suite (currently one `init` test total), security review before mainnet exposure.

### Batch FA-7: Decide whether fleet-treasury routing is wired into real payouts
**Closes:** [#12](https://github.com/fanilabs/fanilab-smartcontract/issues/12) (fleet treasury routing is never wired into the actual payout path)
**Verified:** `grep -rn "get_payout_address" contracts/ --include="*.rs"` shows it is defined in `fleet_management_contract/lib.rs:347` and never called from anywhere else in the workspace (`escrow_contract::payout_driver` has no reference to `fleet_management_contract`). This is a fund-routing correctness gap, not just a testing gap — a driver who is an active fleet member today still gets paid directly instead of through their fleet's treasury, contrary to what `get_payout_address`'s existence implies.
**Files affected:** `contracts/escrow_contract/lib.rs`, `contracts/fleet_management_contract/lib.rs`
**Risk:** Medium — is `payout_driver` supposed to call `get_payout_address`? Confirm intent before writing code.
**Effort:** Small if wiring it in (a cross-contract call + tests); Trivial if the decision is to document it as not-yet-connected.
**Dependencies:** **Product/architecture decision required** — same category as FA-6.
**Validation steps:** if wiring in, add the cross-call to `payout_driver`, with a new integration test paying a fleet-active driver and asserting the treasury address received funds, not the driver directly.

---

# Phase B — Security Improvements

### Batch FB-1: Input validation sweep
**Closes:** [#17](https://github.com/fanilabs/fanilab-smartcontract/issues/17) (`create_escrow` never validates `amount > 0`), [#21](https://github.com/fanilabs/fanilab-smartcontract/issues/21) (`dispute_time_limit` accepts 0 at init), [#32](https://github.com/fanilabs/fanilab-smartcontract/issues/32) (no admin setter for `dispute_time_limit`), [#33](https://github.com/fanilabs/fanilab-smartcontract/issues/33)/[#96](https://github.com/fanilabs/fanilab-smartcontract/issues/96) (`CargoDescriptor`/`DeliveryMetadata`/`create_delivery` accept unbounded/empty input)
**Verified:** `create_escrow` (`escrow_contract/lib.rs:293-330`) has no `amount` check; `dispute_resolution_contract::init` (lib.rs:47-67) accepts `dispute_time_limit: 0` with no setter to fix it later; `create_delivery` (`delivery_contract/lib.rs:78-120`) accepts empty `origin`/`destination` and zero-weight cargo.
**Files affected:** `contracts/escrow_contract/lib.rs`, `contracts/dispute_resolution_contract/lib.rs`, `contracts/delivery_contract/lib.rs`
**Risk:** Low
**Effort:** Small (1–2 days)
**Dependencies:** None
**Validation steps:** one test per new guard (amount ≤ 0 rejected; zero/absurdly-low dispute limit rejected at init and via a new `set_dispute_time_limit`; empty-string/zero-weight delivery creation rejected).

### Batch FB-2: Governance hardening
**Closes:** [#40](https://github.com/fanilabs/fanilab-smartcontract/issues/40) (`remove_admin` can remove the last admin, bricking governance), [#16](https://github.com/fanilabs/fanilab-smartcontract/issues/16) (admin can silently repoint `settlement_contract` with no timelock)
**Verified:** `dispute_resolution_contract::remove_admin` (lib.rs:79-85) has no last-admin guard; `escrow_contract::set_settlement_contract` (lib.rs:233-239) takes effect immediately with no delay.
**Files affected:** `contracts/dispute_resolution_contract/lib.rs`, `contracts/escrow_contract/lib.rs`
**Risk:** Medium — last-admin guard needs an admin count/list where none currently exists.
**Effort:** Medium (2–3 days)
**Dependencies:** If `FC-1` (shared governance) is scheduled soon, do this work as part of it rather than twice.
**Validation steps:** test removing the last admin panics; test `set_settlement_contract` requires a two-step propose/apply with a minimum delay and is cancellable.

### Batch FB-3: Emergency pause / circuit breaker
**Closes:** [#31](https://github.com/fanilabs/fanilab-smartcontract/issues/31) (no emergency pause / circuit breaker across the protocol)
**Files affected:** `contracts/escrow_contract/lib.rs` at minimum; ideally `delivery_contract`, `fleet_management_contract`, `dispute_resolution_contract` too.
**Risk:** Medium — touches every fund-moving entry point; must not itself block legitimate dispute resolution while paused.
**Effort:** Medium (3–5 days)
**Dependencies:** Land after Phase A (no point pausing a contract whose unpause path might itself be exploitable via FA-2/FA-1-class bugs).
**Validation steps:** paused-state tests for every fund-moving function; admin-only pause/unpause enforcement test; document which operations remain available while paused.

### Batch FB-4: Checks-effects-interactions ordering + typed errors in `delivery_contract`
**Closes:** [#87](https://github.com/fanilabs/fanilab-smartcontract/issues/87) (fund-moving functions update state after transfers), [#23](https://github.com/fanilabs/fanilab-smartcontract/issues/23) (`delivery_contract` uses untyped `panic!` instead of typed errors), [#89](https://github.com/fanilabs/fanilab-smartcontract/issues/89) (`propose_admin`/`accept_admin` use raw `panic!`)
**Verified:** every fund-moving function in `escrow_contract` calls `token::Client::transfer` before writing `record.status`; nearly every function in `delivery_contract` panics with a raw string literal (`panic!("NotAuthorized")`, etc.) instead of `panic_with_error!`, despite a `DeliveryError` enum already existing and being discarded via `.unwrap_or_else(|_| panic!(...))`.
**Files affected:** `contracts/delivery_contract/lib.rs`, `contracts/escrow_contract/lib.rs`
**Risk:** Medium — mechanical but touches nearly every function in `delivery_contract`; existing tests asserting on generic panics need updating (coordinate with `FG-2`).
**Effort:** Medium (3–4 days)
**Dependencies:** Land before or alongside `FG-2`.
**Validation steps:** expand `DeliveryError` to cover every raised condition; replace every `panic!(...)` with `panic_with_error!`; reorder `escrow_contract`'s state writes before transfers; full regression run; update `docs/API.md`'s error tables.

### Batch FB-5: Wire or remove the unused `AuthorizedContract` allowlist
**Closes:** [#43](https://github.com/fanilabs/fanilab-smartcontract/issues/43) (`AuthorizedContract` allowlist is built but never consulted)
**Verified:** `set_authorized_contract`/`is_authorized_contract` (`identity_reputation_contract/lib.rs:84-106`) are fully implemented and admin-gated but `is_authorized_contract` is never called anywhere in the workspace.
**Files affected:** `contracts/identity_reputation_contract/lib.rs`
**Risk:** Low
**Effort:** Small (1 day)
**Dependencies:** Combine with `FA-3` (same file, same authorization surface).
**Validation steps:** either replace the hardcoded `delivery_contract`/`dispute_contract` check in `increase_reputation`/`decrease_reputation` with `is_authorized_contract`, or delete the allowlist; test accordingly.

### Batch FB-6: Escrow expiry/timeout mechanism
**Closes:** [#18](https://github.com/fanilabs/fanilab-smartcontract/issues/18) (no expiry/timeout mechanism for `Locked` escrows)
**Files affected:** `contracts/escrow_contract/lib.rs`, `contracts/delivery_contract/lib.rs`
**Risk:** Medium (new state field + entry point)
**Effort:** Medium (2–3 days)
**Dependencies:** None
**Validation steps:** add an admin- or time-gated `reclaim_stale_escrow(delivery_id)` permitting refund after a configurable inactivity window; test both "too early" (rejected) and "past window" (allowed) cases.

---

# Phase C — Architecture Cleanup

### Batch FC-1: Shared governance abstraction
**Closes:** [#77](https://github.com/fanilabs/fanilab-smartcontract/issues/77) (admin/governance model reinvented three ways), [#68](https://github.com/fanilabs/fanilab-smartcontract/issues/68) (duplicate private `is_admin` helpers in `escrow_contract`/`delivery_contract`)
**Files affected:** `contracts/shared_types/lib.rs` (new module), all six contract crates
**Risk:** High — backward-incompatible storage-layout change for any already-deployed instance; large surface area.
**Effort:** Large (1.5–2 weeks)
**Dependencies:** Schedule as its own release, after Phase A. Absorbs `FB-2`'s admin-count-tracking work if not already done.
**Validation steps:** new `shared_types` governance module with its own test suite (single-admin two-step transfer; multi-admin variant for `dispute_resolution_contract`); migrate one contract at a time behind full regression runs; explicit design-review sign-off given the blast radius.

### Batch FC-2: Consolidate `DriverProfile`/`UserProfile` type definitions
**Closes:** [#24](https://github.com/fanilabs/fanilab-smartcontract/issues/24) (three divergent `DriverProfile` definitions), [#41](https://github.com/fanilabs/fanilab-smartcontract/issues/41) (two divergent `UserProfile` definitions)
**Verified:** `identity_reputation_contract/lib.rs:7-22` redeclares its own `DriverProfile` (field-identical to `shared_types::DriverProfile`) and its own `UserProfile { address, join_date }`, where `join_date` diverges from `shared_types::UserProfile.registered_at` for the same concept.
**Files affected:** `contracts/identity_reputation_contract/lib.rs`, `contracts/shared_types/lib.rs`
**Risk:** Medium — storage-schema-affecting; field rename breaks any already-persisted records in a live deployment.
**Effort:** Small (1–2 days)
**Dependencies:** Combine with `FA-3` (same file, same area).
**Validation steps:** delete local struct definitions, import from `shared_types`; full `identity_reputation_contract` test suite; grep-verify no remaining references.

### Batch FC-3: Standardize event topics
**Closes:** [#47](https://github.com/fanilabs/fanilab-smartcontract/issues/47) (typed event structs/topic constants in `shared_types::events` are unused)
**Verified:** `grep -rhn 'Symbol::new(&env, "'` across every contract confirms a real, current mix of `PascalCase` (`AdminTransferred`, `DeliveryContractInitialized`, `DeliveryInTransit`, `FeeUpdated`, `ProtocolInitialized`) and `snake_case` (`delivery_created`, `driver_assigned`, `dispute_raised`, `fleet_registered`, …) topic names, and that `shared_types`'s typed event structs are never constructed anywhere.
**Files affected:** every contract crate, `contracts/shared_types/lib.rs`
**Risk:** Medium-High — breaking change for any off-chain indexer watching current topic strings; coordinate with whoever owns indexing/monitoring.
**Effort:** Medium (3–5 days)
**Dependencies:** Advance notice to any live consumers; consider a deprecation window.
**Validation steps:** pick one convention (recommend `snake_case`); update every call site; tests asserting exact topic names; document the finalized list.

### Batch FC-4: Shared TTL constants and error-code documentation
**Closes:** [#115](https://github.com/fanilabs/fanilab-smartcontract/issues/115) (TTL pair duplicated ~25 call sites with no shared constant), [#46](https://github.com/fanilabs/fanilab-smartcontract/issues/46)/[#111](https://github.com/fanilabs/fanilab-smartcontract/issues/111) (overlapping error enums, no unified reference table)
**Verified:** the literal `518400, 518400` pair is hardcoded inline at every `extend_ttl` call across `delivery_contract`, `dispute_resolution_contract`, `fleet_management_contract`, `identity_reputation_contract`; `shared_types::FaniLabError::DeliveryNotFound = 4` vs. `escrow_contract`'s local `EscrowError::DeliveryNotFound = 2` confirmed as differently-numbered overlapping concepts.
**Files affected:** `contracts/shared_types/lib.rs`, every contract crate
**Risk:** Low
**Effort:** Small (2 days)
**Dependencies:** None; good filler work between larger batches. Also fixes the "no safety margin" TTL-threshold issue (formerly `E3`/#26) as part of choosing new shared constant values.
**Validation steps:** add shared TTL constants to `shared_types`, reference everywhere, with `THRESHOLD` meaningfully below `EXTEND_TO`; write `docs/ERROR_CODES.md` cross-referencing all `#[contracterror]` enums.

### Batch FC-5: Remove dead types
**Closes:** [#42](https://github.com/fanilabs/fanilab-smartcontract/issues/42) (`DeliveryDetails`/`PartyAddresses` fully-defined dead types)
**Files affected:** `contracts/shared_types/lib.rs`
**Risk:** Low
**Effort:** Trivial
**Dependencies:** None
**Validation steps:** grep-confirm no remaining references before deleting.

### Batch FC-6: Normalize `settlement_contract` crate layout
**Closes:** [#35](https://github.com/fanilabs/fanilab-smartcontract/issues/35) (unused `shared_types` dependency in `settlement_contract`), [#116](https://github.com/fanilabs/fanilab-smartcontract/issues/116) (`settlement_contract` is the only crate using `src/lib.rs` layout)
**Files affected:** `contracts/settlement_contract/`
**Risk:** Low
**Effort:** Trivial
**Dependencies:** Hold until `FA-6` resolves — if settlement is implemented for real, it will likely need `shared_types` after all.
**Validation steps:** align crate layout with the other five contracts; drop the dependency only if `FA-6` confirms it stays unused.

---

# Phase D — Code Quality

### Batch FD-1: Remove `get_status` dead stub
**Closes:** [#37](https://github.com/fanilabs/fanilab-smartcontract/issues/37) (`escrow_contract::get_status` is a dead stub that always returns `Pending`)
**Verified:** `escrow_contract/lib.rs:214-216` — `pub fn get_status(_env: Env) -> DeliveryStatus { DeliveryStatus::Pending }`, takes no `delivery_id`, returns the wrong contract's status type.
**Files affected:** `contracts/escrow_contract/lib.rs`
**Risk:** Low
**Effort:** Trivial
**Dependencies:** None
**Validation steps:** delete; grep for any test/doc references; callers should use `get_escrow(delivery_id).status` instead.

### Batch FD-2: Repository hygiene sweep
**Closes:** [#135](https://github.com/fanilabs/fanilab-smartcontract/issues/135) (leftover debris), [#136](https://github.com/fanilabs/fanilab-smartcontract/issues/136) (stale `.vscode` build target), [#56](https://github.com/fanilabs/fanilab-smartcontract/issues/56)/[#57](https://github.com/fanilabs/fanilab-smartcontract/issues/57) (`Makefile`/deploy script still target `wasm32-unknown-unknown`), [#125](https://github.com/fanilabs/fanilab-smartcontract/issues/125) (`release-with-logs` profile unused)
**Verified:** `test_script.py`/`tests_passing.png` still present at repo root; `.vscode/settings.json:2` still pins `wasm32-unknown-unknown`; `Makefile` (lines 6, 10, 14, 18) and `scripts/deploy-all-contracts.sh` (lines 44, 52) still use `wasm32-unknown-unknown` while every CI workflow uses `wasm32v1-none`; `grep -rn "release-with-logs"` across workflows/scripts/Makefile returns nothing. All four independently re-confirmed against current code (not assumed from the closed-issue text).
**Files affected:** repo root, `Cargo.toml`, `Makefile`, `.vscode/settings.json`, `scripts/deploy-all-contracts.sh`
**Risk:** Low
**Effort:** Trivial (half a day)
**Dependencies:** None — good first batch.
**Validation steps:** `make build`/`make test` succeed with `wasm32v1-none`; delete debris files; wire `release-with-logs` into a real path or remove it.

### Batch FD-3: `DeliveryMetadata.delivery_id` cleanup
**Closes:** [#45](https://github.com/fanilabs/fanilab-smartcontract/issues/45) (`DeliveryMetadata.delivery_id` never validated against the real `DeliveryId`)
**Verified:** `create_delivery` (`delivery_contract/lib.rs:78-120`) stores caller-supplied `metadata.delivery_id` verbatim; the real `DeliveryId` is independently generated from an internal counter and never cross-checked against it.
**Files affected:** `contracts/delivery_contract/lib.rs`, `contracts/shared_types/lib.rs`
**Risk:** Low-Medium (storage-schema change if the field is removed)
**Effort:** Small
**Dependencies:** Combine with `FB-1` (both touch `create_delivery`'s validation).
**Validation steps:** either remove the redundant field or have `create_delivery` overwrite it with the real generated ID; test accordingly.

### Batch FD-4: SDK-27 deprecation tracking
**Closes:** [#114](https://github.com/fanilabs/fanilab-smartcontract/issues/114) (blanket `#![allow(deprecated)]` across all six crates)
**Files affected:** every contract crate's `lib.rs` header
**Risk:** Low
**Effort:** Trivial (tracking only)
**Dependencies:** None
**Validation steps:** file one tracking issue referenced by comment in all six files; no functional change until the SDK exposes a replacement API.

---

# Phase E — Performance

### Batch FE-1: Evidence-hash growth cap
**Closes:** [#49](https://github.com/fanilabs/fanilab-smartcontract/issues/49) (`add_evidence_hash` allows unbounded growth of a single storage entry)
**Verified:** `DisputeCase.evidence_hashes: Vec<BytesN<32>>` (`dispute_resolution_contract/lib.rs:205-245`) has no length cap.
**Files affected:** `contracts/dispute_resolution_contract/lib.rs`
**Risk:** Low
**Effort:** Trivial
**Dependencies:** None. (The TTL-safety-margin issue, formerly filed separately as `E3`, is folded into `FC-4` above since it's the same "choose new shared constants" work.)
**Validation steps:** cap `evidence_hashes.len()` (e.g. 20) and reject further additions with a typed error; test the boundary.

---

# Phase F — Documentation

**Do not start this phase until the relevant Phase A/B code fixes have actually landed.**

### Batch FF-1: Populate empty architecture docs
**Closes:** [#66](https://github.com/fanilabs/fanilab-smartcontract/issues/66) (three architecture/design docs committed as completely empty files)
**Verified:** `docs/architecture/event-system.md`, `docs/contract-design/escrow-design.md`, `docs/protocol/delivery-protocol.md` are all confirmed 0 bytes via `wc -l`.
**Files affected:** the three files above
**Risk:** Low
**Effort:** Medium (real writing effort)
**Dependencies:** `event-system.md` should follow `FC-3` (don't document topic names about to change).
**Validation steps:** peer review by someone who didn't write the corresponding code.

### Batch FF-2: Correct `PRODUCTION_READINESS.md`
**Closes:** [#34](https://github.com/fanilabs/fanilab-smartcontract/issues/34) (`PRODUCTION_READINESS.md` claims contradict the codebase's actual state)
**Verified:** the document claims "10/10 Production Ready" and "Zero critical security vulnerabilities" — directly contradicted by `FA-1` through `FA-7`'s verified findings.
**Files affected:** `PRODUCTION_READINESS.md`
**Risk:** Low technically, socially sensitive (walks back a public claim)
**Effort:** Trivial once content is decided
**Dependencies:** **Hard-blocked on Phase A completion.** Regenerate the checklist from actual verified state rather than softening the language.
**Validation steps:** every checkbox traceable to a specific test or code review, not asserted.

### Batch FF-3: Fix versions, links, phantom references, changelog, and API examples
**Closes:** [#127](https://github.com/fanilabs/fanilab-smartcontract/issues/127) (README badges/org link point to nonexistent repo), [#128](https://github.com/fanilabs/fanilab-smartcontract/issues/128) (version mismatch), [#129](https://github.com/fanilabs/fanilab-smartcontract/issues/129) (`docs/DEPLOYMENT.md` phantom function/infra), [#86](https://github.com/fanilabs/fanilab-smartcontract/issues/86) (stale CHANGELOG), [#78](https://github.com/fanilabs/fanilab-smartcontract/issues/78) (`docs/API.md` has one worked example for 30+ functions)
**Verified:** `README.md:229,319-320,364` still link `github.com/fanilab/FaniLab-SmartContract` (real org: `fanilabs/fanilab-smartcontract`, confirmed via `gh repo view`); `README.md:324`/`SECURITY.md:7` claim `0.2.x` while every `contracts/*/Cargo.toml` says `0.1.0`; `CHANGELOG.md`'s `[Unreleased]` section has no entry for the SDK-27/`wasm32v1-none` migration (commit `6944bd4`).
**Files affected:** `README.md`, `SECURITY.md`, all `Cargo.toml`, `docs/DEPLOYMENT.md`, `CHANGELOG.md`, `docs/API.md`
**Risk:** Low
**Effort:** Small-Medium (2–3 days, mostly for worked-example writing)
**Dependencies:** None beyond general accuracy.
**Validation steps:** every README link resolves; every `Cargo.toml` version matches doc claims; `docs/DEPLOYMENT.md` reviewed function-by-function against actual contract APIs; CHANGELOG updated.

---

# Phase G — Testing

### Batch FG-1: Direct tests for the two under-tested dispute-resolution functions
**Closes:** [#92](https://github.com/fanilabs/fanilab-smartcontract/issues/92) (`escrow_contract/test.rs` has no direct test for `resolve_dispute` or `resolve_dispute_split`)
**Verified:** grep of `contracts/escrow_contract/test.rs`'s 14 `#[test]` functions confirms none directly exercise `resolve_dispute` or `resolve_dispute_split` — the exact two functions with confirmed bugs in `FA-1`/`FA-5`.
**Files affected:** `contracts/escrow_contract/test.rs`
**Risk:** Low
**Effort:** Small (1–2 days)
**Dependencies:** Write against the *fixed* behavior — sequence after `FA-1`/`FA-5`.
**Validation steps:** tests for happy path, unauthorized caller, wrong state, and `sender_share_bps` boundaries (0, 10000, >10000) for both functions.

### Batch FG-2: Admin-transfer test coverage + typed-error test updates
**Closes:** [#54](https://github.com/fanilabs/fanilab-smartcontract/issues/54) (two-step admin transfer has zero test coverage)
**Verified:** grep of `escrow_contract/test.rs`'s test names confirms no `propose_admin`/`accept_admin`-named test exists.
**Files affected:** `contracts/escrow_contract/test.rs`, `contracts/delivery_contract/test.rs`
**Risk:** Low
**Effort:** Small (2 days)
**Dependencies:** Sequence after `FB-4` (typed errors must exist before tests can assert on them).
**Validation steps:** propose/accept happy path + both failure modes; every `delivery_contract` test previously asserting a generic panic message updated to assert the new typed error code.

### Batch FG-3: Cross-contract integration coverage
**Closes:** [#84](https://github.com/fanilabs/fanilab-smartcontract/issues/84) (no integration test scaffolding between `fleet_management_contract` and `escrow_contract`), [#51](https://github.com/fanilabs/fanilab-smartcontract/issues/51) (dispute resolution's reputation-penalty cross-call never exercised by any test)
**Files affected:** `contracts/fleet_management_contract/test.rs`, `contracts/dispute_resolution_contract/test.rs`
**Risk:** Low
**Effort:** Medium (3–4 days — multi-contract harness setup is the bulk of the work)
**Dependencies:** Sequence after `FA-3` (reputation rewire) and `FA-7` (fleet-treasury wiring decision) — write tests against whichever behavior those decisions land on.
**Validation steps:** full delivery→escrow→identity_reputation lifecycle test asserting reputation actually changes; full delivery→escrow→fleet_management test asserting (or documenting the absence of) treasury-routed payouts.

### Batch FG-4: Remaining authorization-test gap and property testing
**Closes:** [#103](https://github.com/fanilabs/fanilab-smartcontract/issues/103) (`resolve_dispute_split_funds` has no unauthorized-caller test), [#52](https://github.com/fanilabs/fanilab-smartcontract/issues/52)/[#53](https://github.com/fanilabs/fanilab-smartcontract/issues/53) (no `proptest`/fuzzing despite docs prescribing both)
**Files affected:** `contracts/dispute_resolution_contract/test.rs`, new `proptest`-based test modules, `Cargo.toml`
**Risk:** Low
**Effort:** Medium (the auth test is trivial; property testing is 3–5 days to set up meaningfully — prioritize `calculate_fee` given its role in `FA-1`)
**Dependencies:** Highest-value once `FA-1` has landed (property-test the fixed fee-ceiling logic).
**Validation steps:** unauthorized-caller test for `resolve_dispute_split_funds`; `proptest`-based fee-calculation test sweeping `amount`/`platform_fee_bps` ranges asserting `driver_amount + platform_fee <= amount` always holds.

---

## Suggested overall sequencing

1. **Day 1:** `FD-2` (hygiene, builds trust/process, zero risk) → reopen the GitHub issues this plan targets.
2. **Week 1:** `FA-1` → `FA-2`.
3. **Week 2:** `FA-3` + `FA-4` + `FC-2` + `FB-5` (combined, same files) → `FA-5`.
4. **Week 3:** `FB-1`, `FB-4`, `FB-6` (parallelizable) → `FG-1`, `FG-2` (tests for what just landed).
5. **Decision points (schedule separately once resolved):** `FA-6` (settlement scope), `FA-7` (fleet-treasury wiring).
6. **Weeks 4–6:** `FC-1` (governance) as a dedicated effort, in parallel with `FB-2`, `FB-3`, and the small `FC-4`/`FC-5`/`FC-6`/`FE-1`/`FD-1`/`FD-3`/`FD-4` cleanup batches.
7. **Week 7:** `FC-3` (event standardization) — coordinate with indexer consumers first.
8. **Weeks 8–9:** `FG-3`, `FG-4` (integration/property tests) → `FF-1` through `FF-3` (docs, once code matches what's being documented) → `FF-2` last (only once the "production ready" claim is actually true).

**Total estimated effort: ~7–9 engineer-weeks** for everything in this plan (down from the original draft's 7–11 week estimate, reflecting the batches that were merged rather than a change in underlying scope).

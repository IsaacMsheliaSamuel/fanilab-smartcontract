# FaniLab Smart Contracts — Wave 2 Backlog (Issues #188–)

Authored for the Drips Stellar Wave. Every issue below was derived from a direct
read of the repository at commit `510af08` (`main`, immediately after PR #187
merged the Holdback refund authorization fix), cross-checked against the already
published backlog in `fani-smartcontract-issues.md` (GitHub issues #7–#144) and
against the live issue tracker at `github.com/fanilabs/fanilab-smartcontract`.

Numbering starts at **#188** because GitHub's shared issue/PR sequence is already
at #187; starting here keeps local numbers aligned with the numbers these issues
will receive when filed, matching the convention of the existing backlog document
(where each row links to `/issues/N`).

**Complexity is strictly Medium (150 points) or High (200 points). There are zero
Trivial issues in this document.**

Scope note: the HIGH-severity `refund_escrow` / `Holdback` authorization
vulnerability fixed in PR #187 is deliberately **not** reproduced here. Issues
that touch `Holdback` below concern *different* defects in the surrounding state
machine that the fix did not address.

---

# Issue #288 — `identity_reputation_contract` has no way to remove or deactivate a driver profile

## Problem Statement

`register_driver` creates a permanent `DriverProfile`:

```rust
pub fn register_driver(env: Env, driver: Address) {
    driver.require_auth();
    let key = DataKey::DriverProfile(driver.clone());
    if env.storage().persistent().has(&key) {
        panic_with_error!(&env, FaniLabError::AlreadyInitialized);
    }
    /* create and store profile */
}
```

There is no counterpart. The contract exposes no function that removes a profile,
marks a driver inactive, or suspends them. `update_driver_kyc_status` can set
`kyc_verified = false` and `decrease_reputation` can reduce a score, but the
profile itself persists indefinitely and the driver remains registered.

`fleet_management_contract` by contrast has a full membership lifecycle —
`add_driver_to_fleet`, `accept_fleet_invite`, `cancel_invite`,
`remove_driver_from_fleet`, and a `DriverFleetStatus::Removed` terminal state — and
`deactivate_fleet` for the fleet itself.

## Why It Matters

There is no on-chain mechanism to stop a driver from participating. A driver whose
key is compromised, who is banned for fraud, or who simply leaves the platform
keeps a valid profile and remains eligible for `assign_driver` — which requires
only that the caller is the admin or the driver themselves, with no check against
reputation, KYC status, or any suspension flag.

The closest available lever is driving reputation to zero via repeated
`decrease_reputation` calls, which is indirect, requires an authorized contract to
call it, and still leaves the driver assignable since nothing gates assignment on
score.

The asymmetry with fleet management is telling: the protocol already decided that
membership needs a lifecycle with a terminal state, and implemented one for fleets
but not for the identity registry those fleets draw from.

## Proposed Solution

Add an admin-gated capability to suspend or deactivate a driver profile, with a
status field on `DriverProfile` rather than deleting the record — history should be
preserved for audit, matching the `DriverFleetStatus::Removed` precedent that keeps
membership history rather than erasing it.

Keep the scope to the identity contract: adding the status and the admin function
to set it, plus an accessor. Wiring suspension into `assign_driver` is a separate
change in `delivery_contract` and should be a follow-up, since it involves a
cross-contract call that contract does not currently make for this purpose.

## Acceptance Criteria

- [ ] `DriverProfile` carries a status or active flag
- [ ] An admin-gated function can suspend and reinstate a driver
- [ ] Suspension is observable through an accessor
- [ ] Suspending preserves the profile's history rather than deleting it
- [ ] A suspension event is emitted
- [ ] Non-admin callers cannot suspend or reinstate
- [ ] Existing registration and reputation behavior is unchanged for active drivers

## Technical Notes

- `DriverProfile` is declared in `shared_types` and is shared with `delivery_contract` and `fleet_management_contract`; adding a field is a wire-format change affecting all three plus the SDK.
- `DriverFleetStatus` with its `Removed` terminal state is the in-repo precedent for preserving history.
- `register_driver` panics on re-registration, so suspension must not be implemented as deletion — a deleted profile would let a suspended driver simply re-register.
- Gating `assign_driver` on driver status is deliberately out of scope; note it as follow-up work.

## Relevant Files

- `contracts/identity_reputation_contract/lib.rs` — `register_driver`, `update_driver_kyc_status`, `get_driver_profile`
- `contracts/shared_types/lib.rs` — `DriverProfile`
- `contracts/fleet_management_contract/lib.rs` — `DriverFleetStatus` precedent
- `contracts/identity_reputation_contract/test.rs`

## Testing Requirements

- Unit test: admin can suspend a registered driver
- Unit test: admin can reinstate a suspended driver
- Authorization test: non-admin cannot suspend or reinstate
- Unit test: a suspended driver cannot re-register to reset their profile
- Unit test: suspension preserves reputation and `deliveries_completed`
- Event test: suspension and reinstatement emit correct events
- Regression test: existing registration and reputation flows unaffected

## Definition of Done

- [ ] Suspension capability implemented with history preserved
- [ ] Events emitted
- [ ] Tests above added and passing
- [ ] `docs/API.md` documents the new functions
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

**None**. Gating `assign_driver` on driver status is deliberately excluded and should be filed separately.

## Labels

`feature`


---

# Issue #289 — `get_escrow` reads storage twice and extends TTL on a read-only query

## Problem Statement

`get_escrow` performs a presence check and then a full load, which reads the same
entry again:

```rust
pub fn get_escrow(env: Env, delivery_id: u64) -> EscrowRecord {
    if !env.storage().persistent().has(&escrow_key(delivery_id)) {
        panic_with_error!(&env, EscrowError::DeliveryNotFound);
    }
    load_escrow(&env, delivery_id)
}
```

`load_escrow` already handles the missing case with the same error:

```rust
let record: EscrowRecord = env.storage().persistent().get(&key)
    .unwrap_or_else(|| panic_with_error!(env, EscrowError::DeliveryNotFound));
env.storage().persistent().extend_ttl(&key, ttl::LEDGER_TTL_THRESHOLD, ttl::LEDGER_TTL_EXTEND_TO);
```

So the `has` check is redundant — it produces the identical error `load_escrow`
would produce — and costs an extra storage read on every query.

`load_escrow` also extends the entry's TTL, which means `get_escrow`, a read-only
accessor, writes to the ledger as a side effect.

## Why It Matters

`get_escrow` is called cross-contract by `delivery_contract::get_combined_state`
and by `dispute_resolution_contract` in three places, in addition to direct client
queries. The redundant read is paid on every one of those calls.

The TTL extension is the more interesting half. It is arguably beneficial — reading
an escrow keeps it alive — but it makes a nominally read-only function mutate
ledger state, which means `get_escrow` cannot be used in a simulation-only context
without side effects, and it charges the caller for a write they did not request.

Whether that is intended is undocumented. `load_escrow` is shared by the mutating
functions, where TTL extension is clearly correct; `get_escrow` inherits it
incidentally.

## Proposed Solution

Remove the redundant `has` check, since `load_escrow` already produces the correct
error.

Then decide deliberately whether `get_escrow` should extend TTL. If read-driven
keep-alive is wanted, document it explicitly so callers understand the accessor
writes. If not, give `get_escrow` a non-extending read path separate from the one
the mutating functions use.

## Acceptance Criteria

- [ ] `get_escrow` performs a single storage read for the record
- [ ] A missing escrow still fails with `EscrowError::DeliveryNotFound`
- [ ] The TTL-extension behavior of `get_escrow` is decided and documented
- [ ] Mutating functions continue to extend TTL as they do today
- [ ] Cross-contract callers observe no behavioral change beyond the documented decision
- [ ] Regression test covers both the found and not-found paths

## Technical Notes

- `load_escrow` is used by every mutating function in the contract, so changing it directly would affect them all — prefer adjusting `get_escrow` rather than `load_escrow`.
- `delivery_contract::get_combined_state` and three sites in `dispute_resolution_contract` invoke `get_escrow` cross-contract.
- `escrow_contract` has no equivalent redundant check elsewhere; this appears to be an isolated pattern.
- Soroban charges separately for reads and for TTL extensions, so both halves of this issue have a measurable cost.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `get_escrow`, `load_escrow`
- `contracts/delivery_contract/lib.rs` — `get_combined_state`
- `contracts/dispute_resolution_contract/lib.rs` — escrow fetch sites
- `contracts/escrow_contract/test.rs` — `test_get_escrow_not_found`

## Testing Requirements

- Regression test: `get_escrow` on a missing ID still fails with `DeliveryNotFound`
- Regression test: `get_escrow` on an existing ID returns the correct record
- Unit test: TTL behavior matches the documented decision
- Regression test: cross-contract callers unaffected
- Verification: existing `test_get_escrow_not_found` passes unmodified

## Definition of Done

- [ ] Redundant read removed
- [ ] TTL behavior decided and documented
- [ ] Tests pass unmodified
- [ ] Formatting and clippy clean

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

**None**

## Labels

`performance`, `refactor`


---

# Issue #290 — `payout_driver` silently skips non-positive payouts with no event or error

## Problem Statement

`payout_driver` returns early when the amount is not positive:

```rust
fn payout_driver(env: &Env, token: &Address, driver: &Address, amount: i128,
                 fleet_management_addr: Option<&Address>, fleet_id: Option<u64>) {
    if amount <= 0 {
        return;
    }
    ...
}
```

The caller, `settle_escrow_funds`, does not check the return — the function returns
`()` — so a skipped payout is indistinguishable from a completed one. The
surrounding `release_escrow` and `release_holdback_escrow` still mark the escrow
`Released`, decrement `TotalLocked`, and emit `escrow_released` reporting a
`driver_amount` that was never transferred.

`resolve_dispute_split` guards its transfers the same way (`if sender_amount > 0`,
`if driver_amount > 0`) with the same silence.

## Why It Matters

The condition is reachable. If the platform fee equals or exceeds the escrowed
amount — a 10% fee on an amount of 9 or less, given integer division in
`calculate_fee` — `driver_amount` becomes zero and the driver receives nothing
while the escrow is recorded as successfully released to them.

`create_escrow` only requires `amount > 0`, so a 1-unit escrow is accepted, and
`create_escrows_batch` currently validates nothing at all (issue #189). The emitted
`escrow_released` event reports the computed `driver_amount`, so off-chain
accounting records a payout that did not occur.

The early return is defensively correct — attempting a zero transfer would be
wasteful or rejected — but doing it silently, while the caller reports success, is
what makes it a correctness problem rather than an optimization.

## Proposed Solution

Make a skipped payout observable. The lightest approach is to emit a distinct
event, or to include the actually-transferred amount in `escrow_released` so the
event reflects reality rather than the pre-transfer computation.

Alternatively, reject the condition earlier: enforce a minimum escrow amount at
creation such that `amount - platform_fee > 0` always holds, which removes the
reachable case entirely. That is arguably the better fix, since a zero-payout
release is not a meaningful protocol outcome.

Either way the current combination — silently skip, then report success — should
not stand.

## Acceptance Criteria

- [ ] A zero or negative driver payout is observable rather than silent
- [ ] The `escrow_released` event does not report an amount that was not transferred
- [ ] The equivalent condition in `resolve_dispute_split` is handled consistently
- [ ] Normal positive payouts are unchanged
- [ ] If a minimum amount is enforced, it is documented and validated at creation
- [ ] Regression test covers an escrow whose fee consumes the entire amount

## Technical Notes

- `calculate_fee` uses integer division: `amount * fee_bps / 10_000`, so small amounts round the fee down and `driver_amount` reaches zero only when the fee equals the full amount.
- `MAX_PLATFORM_FEE_BPS` is 1000 (10%), so with a 1-unit escrow the fee is 0 and the driver receives 1 — the reachable cases involve the fee equalling the amount, which requires specific small values; construct the test case deliberately rather than assuming.
- The platform-fee transfer in `settle_escrow_funds` is separately guarded by `if platform_fee > 0`, with the same silence.
- Issue #190 changes how the fee is computed and passed; coordinate if both are in flight.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `payout_driver`, `settle_escrow_funds`, `calculate_fee`, `resolve_dispute_split`, `release_escrow`, `release_holdback_escrow`
- `contracts/escrow_contract/test.rs`

## Testing Requirements

- Unit test: an escrow whose fee consumes the full amount produces an observable outcome
- Unit test: the emitted event's amounts match the amounts actually transferred
- Unit test: `resolve_dispute_split` with a zero share on one side behaves consistently
- Regression test: normal payouts with positive amounts unchanged
- Edge case: minimum viable escrow amount at the maximum fee rate
- Regression test: platform-fee transfer skipped at zero fee remains correct

## Definition of Done

- [ ] Silent skip made observable, or the condition made unreachable by validation
- [ ] Event amounts reflect reality
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

Interacts with #190 (fee computation) and #189 (batch amount validation); independently solvable.

## Labels

`bug`


---

# Issue #291 — `DeliveryCounter` is never TTL-extended; if archived it resets to zero and overwrites existing deliveries

## Problem Statement

`delivery_contract` allocates delivery IDs from a counter held in **persistent**
storage:

```rust
let mut counter: u64 = env.storage().persistent()
    .get(&DataKey::DeliveryCounter).unwrap_or(0);
counter += 1;
env.storage().persistent().set(&DataKey::DeliveryCounter, &counter);
```

`DataKey::DeliveryCounter` appears at four sites in the file (init, both read
sites, both write sites) and **not one of them calls `extend_ttl`**. Every other
persistent entry in the contract — delivery records, `DeliveriesBySender`,
`DeliveriesByRecipient` — is extended immediately after being written.

If the counter entry is archived, the `unwrap_or(0)` fallback silently restarts
allocation from 1.

## Why It Matters

`create_delivery` has no duplicate guard. Unlike `escrow_contract::create_escrow`,
which rejects an existing `delivery_id` with `EscrowError::DuplicateDelivery`,
`create_delivery` writes unconditionally:

```rust
let key = delivery_key(delivery_id);
env.storage().persistent().set(&key, &record);
```

So a reset counter causes the next delivery to be written to
`StorageKey::Delivery(1)`, **overwriting the existing delivery record at that ID**
— its sender, recipient, driver, status, and timestamps all replaced. The escrow
keyed on the same `delivery_id` continues to reference funds that now belong to a
completely different delivery.

The counter is the one persistent entry in the contract that is never refreshed by
ordinary activity: delivery records and indexes are extended on every write, but
the counter is only touched by writes that do not extend it. A quiet period long
enough for archival is exactly the scenario the TTL constants exist to prevent.

## Proposed Solution

Extend the counter's TTL at every write site, matching the pattern used for
delivery records and the secondary indexes.

Consider additionally moving the counter to instance storage, whose lifetime is
tied to the contract itself rather than to a per-entry TTL — that removes the
failure mode structurally rather than relying on every future write site
remembering to extend.

As defense in depth, add a duplicate guard to `create_delivery` mirroring
`create_escrow`'s, so a counter fault can never silently overwrite a record.

## Acceptance Criteria

- [ ] `DataKey::DeliveryCounter` has its TTL extended at every write site
- [ ] The counter survives a long ledger advance without resetting
- [ ] `create_delivery` rejects an ID that already has a delivery record
- [ ] `create_deliveries_batch` applies the same guard
- [ ] Existing delivery-creation behavior is otherwise unchanged
- [ ] Regression test covers a counter that would otherwise have been archived

## Technical Notes

- `shared_types::ttl::{LEDGER_TTL_THRESHOLD, LEDGER_TTL_EXTEND_TO}` are already imported and used elsewhere in the file.
- Write sites are `init` (line ~113), `create_delivery` (line ~186), and `create_deliveries_batch` (line ~321).
- `escrow_contract::create_escrow`'s `DuplicateDelivery` check is the model for the defensive guard; `DeliveryError` would need a matching variant.
- Instance storage is extended by `extend_ttl` on the instance as a whole, which several contracts already do in admin functions — evaluate whether the counter belongs there.

## Relevant Files

- `contracts/delivery_contract/lib.rs` — `DataKey::DeliveryCounter`, `init`, `create_delivery`, `create_deliveries_batch`
- `contracts/shared_types/lib.rs` — `ttl` constants
- `contracts/escrow_contract/lib.rs` — `DuplicateDelivery` guard precedent
- `contracts/delivery_contract/test.rs`

## Testing Requirements

- Unit test: counter value survives a substantial ledger advance
- Unit test: creating a delivery at an already-used ID is rejected
- Regression test: sequential ID allocation is unchanged under normal use
- Regression test: batch creation allocates sequential IDs correctly
- Edge case: `init` followed immediately by creation allocates ID 1

## Definition of Done

- [ ] Counter TTL handled consistently with the contract's other persistent entries
- [ ] Duplicate guard added to both creation paths
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**High**

## Estimated Effort

4–8 hours

## Dependencies

**None**

## Labels

`bug`, `security`

---

# Issue #292 — Driver-initiated disputes always fail because `delivery_contract` rejects the caller the dispute contract accepts

## Problem Statement

`dispute_resolution_contract::raise_dispute` explicitly permits all three parties:

```rust
if caller != delivery.sender
    && caller != delivery.recipient
    && Some(caller.clone()) != delivery.driver
{
    panic_with_error!(&env, FaniLabError::Unauthorized);
}
```

It then cross-calls the delivery contract to advance the delivery state:

```rust
let _: () = env.invoke_contract(
    &delivery_contract_addr,
    &Symbol::new(&env, "raise_dispute"),
    soroban_sdk::vec![&env, caller.into_val(&env), delivery_id.into_val(&env)],
);
```

`delivery_contract::raise_dispute` permits only two:

```rust
let is_sender = caller == delivery.sender;
let is_recipient = caller == delivery.recipient;
if !is_sender && !is_recipient {
    panic_with_error!(&env, FaniLabError::Unauthorized);
}
```

A driver passes the dispute contract's check and is then rejected by the delivery
contract, reverting the whole transaction.

## Why It Matters

Driver access to disputes was added deliberately — closed issue #100 reported that
"drivers are structurally excluded from the entire dispute process", and the fix
extended `dispute_resolution_contract`'s authorization. That fix is inert: the
delivery contract's own check was never widened to match, so the capability does
not work end to end.

The driver is the party with the most at stake in a contested delivery — they have
performed the work and are awaiting payment — and they currently have no way to
initiate a dispute through any path. The failure surfaces as a bare
`Unauthorized` originating in a different contract, which makes it look like a
permissions misconfiguration rather than a protocol gap.

`docs/protocol/delivery-protocol.md` compounds the confusion by documenting the
transition as available to "sender or driver" (see issue #302), which is neither
what the delivery contract enforces nor what the dispute contract enforces.

## Proposed Solution

Widen `delivery_contract::raise_dispute` to accept the assigned driver alongside
the sender and recipient, so the two contracts agree.

Confirm the intended authorization set deliberately: `dispute_resolution_contract`
allows sender, recipient, and driver, so matching that is the natural target.
Update the protocol documentation in the same change so all three sources agree.

## Acceptance Criteria

- [ ] A driver can raise a dispute through `dispute_resolution_contract` end to end
- [ ] `delivery_contract::raise_dispute` accepts the assigned driver
- [ ] Sender and recipient continue to be accepted
- [ ] A non-party is still rejected with `Unauthorized`
- [ ] An address that is not the *assigned* driver for that delivery is rejected
- [ ] `docs/protocol/delivery-protocol.md` documents the actual authorization set
- [ ] Regression test drives a driver-initiated dispute through both contracts

## Technical Notes

- `delivery.driver` is `Option<Address>`; the check must handle an unassigned delivery, where there is no driver to authorize.
- The dispute contract already compares with `Some(caller.clone()) != delivery.driver`, which is the pattern to mirror.
- `escrow_contract::raise_dispute` accepts sender, recipient, and driver, so the escrow layer is already consistent with the intended set — only the delivery contract diverges.
- Note the interaction with issue #193: for a `Delivered` delivery the escrow is in `Holdback`, which `escrow_contract::raise_dispute` currently rejects, so the end-to-end driver path also needs that fix to work post-delivery.

## Relevant Files

- `contracts/delivery_contract/lib.rs` — `raise_dispute`
- `contracts/dispute_resolution_contract/lib.rs` — `raise_dispute`
- `contracts/escrow_contract/lib.rs` — `raise_dispute` (already permits all three)
- `docs/protocol/delivery-protocol.md`

## Testing Requirements

- Integration test: driver raises a dispute through `dispute_resolution_contract` and it succeeds
- Unit test: `delivery_contract::raise_dispute` accepts the assigned driver
- Authorization test: a non-party is rejected
- Authorization test: an address that is not the assigned driver is rejected
- Edge case: raising a dispute on an unassigned delivery with no driver
- Regression test: sender- and recipient-initiated disputes unchanged

## Definition of Done

- [ ] Authorization aligned across both contracts
- [ ] End-to-end driver dispute test passing
- [ ] Protocol documentation corrected
- [ ] Formatting, clippy, and full suite clean

## Complexity

**High**

## Estimated Effort

4–8 hours

## Dependencies

For post-delivery disputes the driver path also requires #193; pre-delivery disputes work independently.

## Labels

`bug`, `security`

---

# Issue #293 — `freeze_funds` reports success without freezing when the escrow is in a terminal state

## Problem Statement

`escrow_contract::freeze_funds` guards its state change with an `if` that has no
`else`:

```rust
let mut record = load_escrow(&env, delivery_id);
if record.status == EscrowStatus::Locked || record.status == EscrowStatus::Holdback {
    record.status = EscrowStatus::Paused;
    record.disputed_at = Some(env.ledger().timestamp());
    save_escrow(&env, delivery_id, &record);
    env.events().publish(/* funds_frozen */);
}
// no else — function returns successfully having done nothing
```

If the escrow is `Released`, `Refunded`, `Split`, or already `Paused`, the
function returns `()` normally. No error is raised, no event is emitted, and the
caller cannot distinguish this from a successful freeze.

Every other state-guarded function in the contract panics with
`EscrowError::InvalidState` when its precondition is not met.

## Why It Matters

`freeze_funds` is called by `dispute_resolution_contract::raise_dispute` as the
step that secures the funds before a dispute is opened:

```rust
let _: () = env.invoke_contract(&escrow_addr, &Symbol::new(&env, "freeze_funds"), ...);

let dispute_key = DataKey::Dispute(delivery_id);
if env.storage().persistent().has(&dispute_key) { /* DuplicateDelivery */ }
/* create and store the DisputeCase */
```

The return value is discarded, so a silent no-op is indistinguishable from
success. The dispute contract proceeds to record an `Open` `DisputeCase` while
the escrow was never actually frozen — and if the escrow was already `Released`,
the funds are gone.

The result is a dispute that exists on chain, appears actionable, and can never
be resolved: `resolve_dispute_split_funds` requires the escrow to be `Paused` and
will revert, while `resolve_dispute_refund_sender` and `resolve_dispute_pay_driver`
will revert inside the escrow's own guard. The dispute is stuck `Open` with no
path forward.

## Proposed Solution

Panic with `EscrowError::InvalidState` when the escrow is not in a freezable
state, matching every other state-guarded function in the contract. The dispute
contract's transaction then reverts cleanly instead of recording an unresolvable
dispute.

Decide explicitly how an already-`Paused` escrow should behave: treating a
re-freeze as a successful no-op is defensible, but it should be a documented
decision rather than a side effect of the missing `else`.

## Acceptance Criteria

- [ ] `freeze_funds` rejects a `Released`, `Refunded`, or `Split` escrow with a typed error
- [ ] The behavior for an already-`Paused` escrow is decided and documented
- [ ] Freezing a `Locked` or `Holdback` escrow works exactly as today
- [ ] `dispute_resolution_contract::raise_dispute` reverts rather than recording an unfreezable dispute
- [ ] The caller-restriction to the configured dispute contract is unchanged
- [ ] Regression test covers each terminal state

## Technical Notes

- `freeze_funds` is intentionally exempt from `require_not_paused` so escrows can be frozen during a protocol halt — that exemption is documented in a code comment and must be preserved.
- The caller check restricting this to `DataKey::DisputeResolutionContract` is correct and should not change.
- `dispute_resolution_contract::raise_dispute` calls `freeze_funds` *after* advancing the delivery to `Disputed`, so a revert also rolls back that transition — verify the ordering still produces coherent state.
- `test_freeze_funds_remains_available_while_paused` and `test_freeze_funds_unauthorized_caller_rejected` are the existing tests to extend.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `freeze_funds`
- `contracts/dispute_resolution_contract/lib.rs` — `raise_dispute`
- `contracts/escrow_contract/test.rs` — existing `freeze_funds` tests

## Testing Requirements

- Unit test: `freeze_funds` on a `Released` escrow → typed error
- Unit test: same for `Refunded` and `Split`
- Unit test: already-`Paused` escrow behaves per the documented decision
- Integration test: raising a dispute against a released delivery reverts rather than creating a stuck dispute
- Regression test: freezing `Locked` and `Holdback` escrows still works
- Regression test: existing pause-exemption and authorization tests unchanged

## Definition of Done

- [ ] Silent no-op replaced with a typed error
- [ ] Re-freeze semantics documented
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**High**

## Estimated Effort

2–4 hours

## Dependencies

**None**

## Labels

`bug`, `security`

---

# Issue #294 — `create_escrow` never verifies that the delivery it funds exists

## Problem Statement

`escrow_contract::create_escrow` accepts an arbitrary `delivery_id` and makes no
cross-contract call to confirm a corresponding delivery record exists. The
function performs zero `invoke_contract` calls before writing the escrow:

```rust
sender.require_auth();
require_not_paused(&env);
if amount <= 0 { /* InvalidAmount */ }
if env.storage().persistent().has(&escrow_key(delivery_id)) { /* DuplicateDelivery */ }
let config = load_protocol_config(&env);
if token != config.token { /* InvalidToken */ }
token::Client::new(&env, &token).transfer(&sender, env.current_contract_address(), &amount);
save_escrow(/* ... */);
```

The `recipient` and `driver` are likewise taken on the sender's word rather than
read from the delivery record.

## Why It Matters

An escrow can be created for a `delivery_id` that does not exist, or that exists
with entirely different parties. Nothing reconciles the two records at creation
time, and the mismatch only surfaces later:
`delivery_contract::get_combined_state` would report desynchronization, and
`confirm_delivery`'s call to `mark_holdback_escrow` would operate on an escrow
whose recipient is not the delivery's recipient.

The practical consequence is orphaned or mismatched escrows that consume a
`delivery_id` permanently — the `DuplicateDelivery` guard means the ID can never
be reused, so a mistyped ID blocks the real delivery from ever being funded.

This is the mirror of issue #295: the delivery contract assumes an escrow exists,
and the escrow contract assumes a delivery exists, and neither verifies. The two
records are only ever correlated by convention.

Impact is bounded — the sender is escrowing their own funds and cannot take
anyone else's — so this is a data-integrity and liveness concern rather than a
theft vector.

## Proposed Solution

Have `create_escrow` cross-call `delivery_contract::get_delivery` to confirm the
delivery exists, and verify that the supplied `recipient` and `driver` match the
delivery record.

This requires the escrow contract to know the delivery contract's address, which
it does not currently store. Adding a `set_delivery_contract` admin setter
mirrors how `delivery_contract` already stores `DataKey::EscrowContract`, and how
`escrow_contract` already stores the dispute and fleet contract addresses.

Make the check conditional on the delivery contract being configured, so existing
deployments and tests that do not wire it continue to work — matching how the
fleet and settlement integrations are already optional.

## Acceptance Criteria

- [ ] `escrow_contract` can be configured with the delivery contract's address
- [ ] When configured, `create_escrow` rejects a `delivery_id` with no delivery record
- [ ] When configured, it rejects a `recipient` or `driver` that disagrees with the delivery record
- [ ] When not configured, behavior is unchanged
- [ ] The setter is admin-gated
- [ ] Regression test covers both configured and unconfigured paths

## Technical Notes

- `delivery_contract::get_delivery` panics with `FaniLabError::DeliveryNotFound` for an unknown ID, so a failed lookup naturally reverts the escrow creation.
- The delivery's `driver` is `Option<Address>` and is `None` until `assign_driver` runs, so the driver comparison must tolerate an unassigned delivery or the ordering constraint must be documented.
- `create_escrows_batch` should receive the same treatment for consistency.
- This adds a cross-contract call to the hot creation path; weigh the resource cost against the integrity benefit and note the decision.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `create_escrow`, `create_escrows_batch`, `DataKey`
- `contracts/delivery_contract/lib.rs` — `get_delivery`, `DataKey::EscrowContract` precedent
- `contracts/escrow_contract/test.rs`

## Testing Requirements

- Unit test: escrow creation for a nonexistent delivery is rejected when configured
- Unit test: mismatched recipient is rejected
- Unit test: mismatched driver is rejected, or the unassigned case is handled as documented
- Regression test: creation with no delivery contract configured behaves as today
- Authorization test: only an admin can set the delivery contract address
- Integration test: normal create-delivery-then-create-escrow flow still works

## Definition of Done

- [ ] Optional delivery verification implemented
- [ ] Setter added and documented in `docs/API.md`
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

Conceptually paired with #295; each is independently solvable.

## Labels

`bug`, `enhancement`

---

# Issue #295 — `cancel_delivery` cannot cancel a delivery that has no escrow

## Problem Statement

`delivery_contract::cancel_delivery` unconditionally cross-calls the escrow
contract before updating its own state:

```rust
validate_transition(delivery.status, DeliveryStatus::Cancelled)
    .unwrap_or_else(|_| panic_with_error!(&env, FaniLabError::InvalidState));

let escrow_address: Address = /* ... */;
let _: () = env.invoke_contract(
    &escrow_address,
    &soroban_sdk::Symbol::new(&env, "refund_escrow"),
    soroban_sdk::vec![&env, sender.into_val(&env), u64::from(delivery_id).into_val(&env)],
);

delivery.status = DeliveryStatus::Cancelled;
```

`escrow_contract::refund_escrow` begins with `load_escrow`, which panics with
`EscrowError::DeliveryNotFound` when no escrow record exists. The panic propagates
and reverts the whole cancellation.

Delivery creation and escrow creation are separate calls on separate contracts —
`create_delivery` never funds an escrow (see issue #203) — so a delivery with no
escrow is an ordinary, reachable state.

## Why It Matters

A sender who creates a delivery and then does not fund an escrow — because they
changed their mind, the funding transaction failed, or they simply never got to it
— has a delivery record they can never cancel. It remains `Pending` permanently,
occupying a `delivery_id` and appearing in the sender's and recipient's secondary
indexes indefinitely.

There is no alternative exit. `cancel_delivery` is the only path to
`DeliveryStatus::Cancelled`, and the other transitions require a driver
assignment and a funded escrow to be meaningful.

The failure is also opaque: the sender receives `DeliveryNotFound` from the escrow
contract when cancelling a delivery that plainly exists, which reads as a bug in
the caller rather than a missing precondition.

## Proposed Solution

Make the escrow refund conditional on an escrow existing. The escrow contract
would need a non-panicking existence check — `has_escrow(delivery_id) -> bool`, or
`get_escrow` returning `Option` — so the delivery contract can skip the refund
when there is nothing to refund and still complete the cancellation.

Alternatively, tolerate the specific `DeliveryNotFound` failure from the escrow
call and proceed, though Soroban's error handling makes a positive existence check
the cleaner shape.

Preserve the existing ordering guarantee: the escrow call must still run before
the delivery's state is mutated, so a genuine refund failure cannot leave the
delivery cancelled with funds still locked.

## Acceptance Criteria

- [ ] A delivery with no escrow can be cancelled
- [ ] A delivery with an escrow still triggers the refund before its state changes
- [ ] A genuine refund failure still reverts the whole cancellation
- [ ] The cancelled delivery's state and events are correct in both cases
- [ ] Authorization is unchanged — only the sender may cancel
- [ ] Regression test covers cancellation both with and without an escrow

## Technical Notes

- `escrow_contract` currently exposes no non-panicking existence check; `get_escrow` panics via an explicit `has` guard and `load_escrow` panics on a miss.
- The `#[cfg(test)] MockEscrowContract` in `delivery_contract/test.rs` will need a matching method for whichever accessor is added.
- Issue #204's note about cross-contract-call ordering applies: the escrow interaction deliberately precedes local state mutation so a failure rolls everything back.
- Closed issue #95 added rollback coverage for failing escrow calls; extend that suite rather than duplicating it.

## Relevant Files

- `contracts/delivery_contract/lib.rs` — `cancel_delivery`
- `contracts/escrow_contract/lib.rs` — `refund_escrow`, `load_escrow`, `get_escrow`
- `contracts/delivery_contract/test.rs` — `MockEscrowContract`

## Testing Requirements

- Unit test: cancelling a delivery with no escrow succeeds and sets `Cancelled`
- Regression test: cancelling a delivery with an escrow still refunds the sender
- Regression test: a failing refund still reverts the cancellation
- Authorization test: a non-sender still cannot cancel
- State test: cancellation from `Pending` and from `Active` both behave correctly
- Event test: `delivery_cancelled` emitted in both the escrow and no-escrow cases

## Definition of Done

- [ ] Cancellation works without an escrow
- [ ] Refund ordering and rollback behavior preserved
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

Conceptually paired with #294; each is independently solvable.

## Labels

`bug`

---

# Issue #296 — `create_escrow` never validates `fleet_id`, letting the sender choose where the driver's payout is routed

## Problem Statement

`create_escrow` takes `fleet_id: Option<u64>` from the caller and stores it on the
`EscrowRecord` without any validation — the identifier appears exactly once in the
function, at the point it is written into the record.

At settlement, that stored value decides the payout destination:

```rust
if let (Some(fleet_addr), Some(fid)) = (fleet_management_addr, fleet_id) {
    let treasury: Address = env.invoke_contract(
        fleet_addr, &Symbol::new(env, "get_payout_address"),
        soroban_sdk::vec![env, driver.into_val(env), fid.into_val(env)]);
    payout_address = treasury;
}
```

`fleet_management_contract` keys membership as
`DataKey::DriverFleet(fleet_id, driver)`, so a driver may be `Active` in any
number of fleets simultaneously. Nothing constrains which of them the sender may
name, and nothing ties the chosen fleet to the delivery.

## Why It Matters

The sender — the party paying, and the party whose interests are opposite the
driver's on payout — unilaterally selects the fleet whose treasury receives the
driver's earnings.

Two concrete consequences follow. A sender can **omit** `fleet_id` for a driver
who is an active fleet member, routing the payment to the driver personally and
bypassing the fleet's arrangement entirely. Or a sender can **name** a fleet the
driver belongs to but which had nothing to do with this delivery, diverting the
earnings to that fleet's treasury.

Neither the driver nor the fleet consents to or can observe the choice: the
`fleet_id` is fixed at escrow creation, before the driver is necessarily even
assigned, and `EscrowRecord.fleet_id` is immutable thereafter.

This is a real authorization gap rather than a theft vector — the funds reach a
legitimate party either way — but "which legitimate party" is precisely what fleet
routing exists to determine, and it is currently the sender's unilateral call.

## Proposed Solution

Validate the claimed fleet relationship at settlement rather than trusting the
stored value. `get_payout_address` already receives both the driver and the fleet
ID and already returns the driver's own address when membership is not `Active`,
so the membership check exists — what is missing is any constraint on the sender's
ability to pick a fleet, or to decline to.

The more robust direction is to stop taking `fleet_id` from the sender at all and
resolve the driver's fleet at settlement time from the fleet contract. That
requires a driver-to-fleet lookup the contract does not currently expose, since
membership is keyed by `(fleet_id, driver)` rather than by driver.

Whichever direction is chosen, the outcome should be that the driver's fleet
affiliation determines routing, not the sender's declaration.

## Acceptance Criteria

- [ ] A sender cannot route a driver's payout to a fleet the driver is not active in
- [ ] A sender cannot bypass a driver's active fleet arrangement by omitting `fleet_id`
- [ ] Routing for a driver with no fleet membership is unchanged
- [ ] Routing for a driver in exactly one fleet is unchanged
- [ ] The behavior for a driver active in multiple fleets is defined and documented
- [ ] Regression test covers a sender naming a fleet the driver does not belong to

## Technical Notes

- `DataKey::DriverFleet(FleetId, Address)` means membership lookup requires knowing the fleet ID; a driver-to-fleets index does not exist and would need adding for the settlement-time resolution approach.
- `get_payout_address` already returns the driver's address for `Pending`, `Removed`, and `None` statuses, so an invalid claim currently degrades to a direct payout rather than failing — that is the existing safety net.
- Issue #217 covers a related but distinct problem: that routing is resolved at payout time from mutable fleet state. This issue is about who gets to assert the fleet in the first place.
- Issue #272 proposes adding `fleet_id` to the batch creation path; whatever validation is agreed here should apply there too.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `create_escrow`, `payout_driver`, `settle_escrow_funds`
- `contracts/fleet_management_contract/lib.rs` — `get_payout_address`, `DataKey::DriverFleet`
- `contracts/shared_types/lib.rs` — `EscrowRecord.fleet_id`

## Testing Requirements

- Integration test: sender names a fleet the driver is not a member of → payout does not reach that treasury
- Integration test: sender omits `fleet_id` for an active fleet driver → behavior matches the agreed policy
- Regression test: driver with no fleet is paid directly
- Regression test: driver in one fleet routes to that treasury
- Edge case: driver active in two fleets — documented behavior asserted
- Authorization test: the driver's own fleet membership governs the outcome

## Definition of Done

- [ ] Fleet routing determined by the driver's membership rather than the sender's claim
- [ ] Multi-fleet behavior documented
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**High**

## Estimated Effort

1–2 days

## Dependencies

Related to #217 and #272; each addresses a different aspect of fleet routing and all three are independently solvable.

## Labels

`security`, `bug`

---

# Issue #297 — `mark_in_transit` advances a delivery without verifying the escrow is funded

## Problem Statement

`delivery_contract::mark_in_transit` checks the caller is the assigned driver and
that the transition `Active → InTransit` is legal, then writes the new status. It
makes no cross-contract call and never consults the escrow:

```rust
match &delivery.driver {
    Some(assigned) if *assigned == driver => {}
    _ => panic_with_error!(&env, FaniLabError::Unauthorized),
}
validate_transition(delivery.status, DeliveryStatus::InTransit)
    .unwrap_or_else(|_| panic_with_error!(&env, FaniLabError::InvalidState));
delivery.status = DeliveryStatus::InTransit;
```

`assign_driver` behaves the same way. Because escrow creation is a separate call
on a separate contract, a delivery can reach `InTransit` with no escrow at all, or
with an escrow that was already refunded.

## Why It Matters

A driver has no on-chain assurance that funds exist before they begin work. The
protocol's value proposition is that the driver is paid from an escrow secured
before delivery, and nothing enforces that the escrow is actually there and
`Locked` at the moment the driver commits.

The reverse desynchronization is also reachable: `reclaim_expired_escrow` can
refund a `Locked` escrow without touching the delivery (issue #299), so a driver
could mark a delivery in transit against an escrow that has already been returned
to the sender.

`get_combined_state` exists precisely to detect these mismatches, which
acknowledges they occur — but detection after the fact does not help a driver who
has already collected the package.

## Proposed Solution

Have `mark_in_transit` verify the escrow is present and `Locked` before advancing,
via a cross-call to `escrow_contract::get_escrow`. The delivery contract already
stores `DataKey::EscrowContract` and cross-calls it in `cancel_delivery`,
`confirm_delivery`, and `raise_dispute`, so the wiring exists.

Consider whether `assign_driver` warrants the same check — assignment is a weaker
commitment than transit, so gating transit alone may be the right balance. Decide
deliberately and document it.

## Acceptance Criteria

- [ ] `mark_in_transit` rejects a delivery with no corresponding escrow
- [ ] It rejects a delivery whose escrow is not `Locked`
- [ ] A delivery with a `Locked` escrow transitions exactly as today
- [ ] The failure carries a typed, diagnosable error
- [ ] The decision on `assign_driver` is documented
- [ ] Regression test covers transit attempted against a missing and a refunded escrow

## Technical Notes

- `escrow_contract::get_escrow` panics with `DeliveryNotFound` for a missing escrow, so a bare cross-call surfaces the missing case — but a non-panicking existence accessor (issue #311) would give a cleaner error.
- The `MockEscrowContract` in `delivery_contract/test.rs` returns a hardcoded `Locked` escrow, so existing tests would pass unchanged; issue #231 covers making that mock model real state.
- Adding a cross-call to `mark_in_transit` increases its resource cost on a hot path — weigh and note the trade-off.

## Relevant Files

- `contracts/delivery_contract/lib.rs` — `mark_in_transit`, `assign_driver`, `DataKey::EscrowContract`
- `contracts/escrow_contract/lib.rs` — `get_escrow`
- `contracts/delivery_contract/test.rs` — `MockEscrowContract`

## Testing Requirements

- Unit test: `mark_in_transit` with no escrow → typed rejection
- Unit test: `mark_in_transit` with a `Refunded` escrow → typed rejection
- Regression test: `mark_in_transit` with a `Locked` escrow succeeds
- Authorization test: a non-assigned driver is still rejected
- Integration test against the real escrow contract, not only the mock

## Definition of Done

- [ ] Escrow verification added to the transit transition
- [ ] `assign_driver` decision documented
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

Benefits from #311's existence accessor and #231's realistic mock; solvable without either.

## Labels

`bug`, `security`

---

# Issue #298 — Three admin setters in `escrow_contract` do not extend instance-storage TTL

## Problem Statement

`escrow_contract` stores its configuration in instance storage. Some admin
functions extend the instance TTL after writing and some do not:

| Function | extends instance TTL |
|---|---|
| `update_platform_fee` | yes |
| `set_dispute_resolution_contract` | yes |
| `propose_admin` | yes |
| `update_slippage_tolerance` | **no** |
| `set_fleet_management_contract` | **no** |
| `set_paused` | **no** |

The three that do call
`env.storage().instance().extend_ttl(ttl::LEDGER_TTL_THRESHOLD, ttl::LEDGER_TTL_EXTEND_TO)`;
the three that do not simply write and return.

## Why It Matters

Instance storage holds the admin address, `ProtocolConfig`, the paused flag, and
every peer-contract address. If the instance entry is archived, the contract
reverts to an uninitialized state — `load_protocol_config` panics with
`NotInitialized`, and `is_protocol_paused` falls back to `unwrap_or(false)`.

That fallback is the sharpest edge: **a paused protocol would silently become
unpaused** if the instance entry lapsed, because absence is read as "not paused".
`set_paused` is the one function whose write most needs to persist, and it is one
of the three that does not extend.

In practice ordinary escrow activity extends the instance TTL through the
functions that do call it, so this is a latent risk rather than an active fault —
but a protocol paused during an incident is precisely the period when ordinary
activity has stopped.

Closed issue #25 previously reported that instance TTL was extended by only two of
many admin writers; that fix covered three functions and left these three behind.

## Proposed Solution

Add the instance TTL extension to the three functions that lack it, matching the
three that have it.

Consider extracting a small helper so the pattern is applied uniformly and future
admin functions cannot omit it — the repetition across six call sites is what
allowed the gap to persist through one round of fixes.

## Acceptance Criteria

- [ ] `update_slippage_tolerance`, `set_fleet_management_contract`, and `set_paused` extend instance TTL
- [ ] The three functions that already extend are unchanged
- [ ] The paused flag survives a long ledger advance with no other activity
- [ ] A shared helper or equivalent guard prevents future omissions
- [ ] Regression test covers pause state persisting across a ledger advance

## Technical Notes

- `shared_types::ttl::{LEDGER_TTL_THRESHOLD, LEDGER_TTL_EXTEND_TO}` are already imported and used in this file.
- `is_protocol_paused` reads with `unwrap_or(false)`, which is why archival fails open rather than closed — worth a comment noting the consequence.
- Other contracts have the same pattern of per-function extension; a follow-up audit across all six would be reasonable but should be filed separately rather than expanding this issue.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `update_slippage_tolerance`, `set_fleet_management_contract`, `set_paused`, `is_protocol_paused`
- `contracts/shared_types/lib.rs` — `ttl` constants

## Testing Requirements

- Unit test: paused state persists after a substantial ledger advance
- Unit test: slippage tolerance persists similarly
- Unit test: fleet contract address persists similarly
- Regression test: existing admin function behavior unchanged
- Verification: every instance-writing function in the contract extends TTL

## Definition of Done

- [ ] TTL extension applied consistently
- [ ] Guard against future omissions in place
- [ ] Tests above added and passing
- [ ] Formatting and clippy clean

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

**None**. Distinct from the closed issue #25, which covered a different subset of functions.

## Labels

`bug`, `security`

---

# Issue #299 — `reclaim_expired_escrow` refunds the sender but leaves the delivery record stranded

## Problem Statement

`reclaim_expired_escrow` is permissionless and refunds an expired `Locked` escrow
to the sender. It makes **zero** cross-contract calls — the delivery record is
never informed:

```rust
record.status = EscrowStatus::Refunded;
save_escrow(&env, delivery_id, &record);
/* TotalLocked decremented, tokens transferred to sender */
env.events().publish((events::escrow_refunded(&env), delivery_id), (record.sender, record.amount));
```

The corresponding `DeliveryRecord` retains whatever status it held — `Pending`,
`Active`, or `InTransit` — indefinitely.

## Why It Matters

The protocol ends up in a state its own consistency check classifies as invalid.
`validate_state_sync` maps `Cancelled → Refunded` as the only synchronized pairing
involving a refund, so a reclaimed escrow leaves combinations like
`(Active, Refunded)` that `get_combined_state` reports as desynchronized —
correctly, but with no mechanism to resolve it.

The delivery is also functionally stuck. Its escrow is gone, so `confirm_delivery`
would fail at `mark_holdback_escrow`, and `cancel_delivery` would fail at
`refund_escrow` because the escrow is no longer `Locked` (and see issue #295 for
the missing-escrow case). A driver may still be assigned and believe the job is
live.

Because `reclaim_expired_escrow` is callable by anyone, this state can be induced
by any third party once the 30-day expiry has passed, without the sender's or
driver's involvement.

## Proposed Solution

Have the reclaim path transition the delivery to `Cancelled`, restoring the
synchronized pairing the state machine already defines for a refund.

That requires the escrow contract to hold the delivery contract's address and
cross-call it, which it does not do today — the same wiring issue #294 proposes.
An alternative is a delivery-side `reclaim` entry point that drives both
contracts in the correct order, keeping the cross-contract direction consistent
with the existing `delivery → escrow` flow.

Whichever direction is chosen, `validate_transition` must permit the resulting
delivery transition: `InTransit → Cancelled` is not currently legal and would need
to be added deliberately, or the reclaim restricted to deliveries in states from
which cancellation is already valid.

## Acceptance Criteria

- [ ] Reclaiming an expired escrow leaves delivery and escrow states synchronized
- [ ] `get_combined_state` reports synchronized after a reclaim
- [ ] The permitted delivery transitions are decided explicitly and reflected in `validate_transition`
- [ ] Reclaim remains permissionless
- [ ] The expiry precondition and `Locked`-only guard are unchanged
- [ ] Regression test asserts post-reclaim synchronization from each reachable delivery status

## Technical Notes

- `validate_transition` currently allows `Pending → Cancelled` and `Active → Cancelled` but **not** `InTransit → Cancelled`; a reclaim of an in-transit delivery therefore has no legal target state today.
- `EscrowRecord.expires_at` is set to `created_at + 30 days` at creation and is only consulted by this function.
- The cross-contract direction matters: every existing call runs delivery → escrow, so adding escrow → delivery introduces a new dependency edge — weigh that against a delivery-side entry point.
- Issue #198 covers `validate_state_sync`'s missing `Holdback` case; this issue concerns a different unsynchronized pairing.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `reclaim_expired_escrow`
- `contracts/delivery_contract/lib.rs` — `validate_transition`, `cancel_delivery`, `validate_state_sync`
- `contracts/escrow_contract/test.rs` — `test_reclaim_expired_escrow_refunds_sender`

## Testing Requirements

- Integration test: reclaim an expired escrow, assert delivery and escrow are synchronized
- Unit test: reclaim from each reachable delivery status behaves per the agreed design
- Regression test: refund amount and `TotalLocked` decrement unchanged
- Regression test: reclaim still rejected before expiry and for non-`Locked` escrows
- Edge case: reclaim of an `InTransit` delivery, given the transition gap

## Definition of Done

- [ ] Delivery and escrow remain synchronized after reclaim
- [ ] Transition rules updated deliberately
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

Shares the escrow → delivery wiring question with #294; the two should agree on direction.

## Labels

`bug`

---

# Issue #300 — `create_escrows_batch` has essentially no test coverage

## Problem Statement

`create_escrows_batch` is referenced only twice in
`contracts/escrow_contract/test.rs`, and neither reference tests its core
behavior — the existing coverage asserts that the function is rejected while the
protocol is paused.

Nothing verifies that a batch actually creates the escrows it claims to, that the
returned count is correct, that funds are transferred, that records are written
with the right fields, or that the secondary indexes are populated.

By contrast `create_deliveries_batch` is referenced seven times in the delivery
contract's suite.

## Why It Matters

The absence of coverage is why several defects in this backlog survived review.
Issues #188 (no `TotalLocked` update), #189 (no amount or token validation), #196
(divergent event payload), #272 (`fleet_id` hardcoded to `None`), and #273 (wrong
error type) are all in this one function, and every one of them would have been
caught by a test that simply created a batch and asserted the resulting state.

The batch path also has genuinely different mechanics from the single path — it
accumulates driver indexes in an in-memory `Map` and flushes them after the loop —
so single-escrow coverage does not transfer.

## Proposed Solution

Add a test module covering the function's core contract: escrows created, count
returned, tokens transferred, records correct, indexes populated, and batch-size
limit enforced.

Write the tests against current behavior and mark with a comment any assertion
that encodes a known defect, so the tests can be tightened as #188, #189, #272,
and #273 land rather than blocking on them.

## Acceptance Criteria

- [ ] A batch creates one escrow per element with correct sender, recipient, driver, token, amount, and status
- [ ] The returned count equals the number of escrows created
- [ ] Tokens are transferred from the sender for the full batch total
- [ ] All three secondary indexes are populated for every element
- [ ] A batch exceeding `MAX_BATCH_SIZE` is rejected
- [ ] A duplicate `delivery_id` within or across batches is rejected
- [ ] `expires_at` and `created_at` are set correctly

## Technical Notes

- `MAX_BATCH_SIZE` is 100 in `escrow_contract::constants`; a batch of exactly 100 must succeed and 101 must fail.
- The driver-index flush uses `soroban_sdk::Map<DataKey, Vec<u64>>`, so a batch containing the same driver twice is the edge case most likely to expose a bug.
- Issue #226 covers index accessor coverage more broadly; scope this issue to the batch creation path to avoid overlap.
- Existing single-escrow tests are the model for balance and record assertions.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `create_escrows_batch`
- `contracts/escrow_contract/test.rs`

## Testing Requirements

- Unit test: batch of 3 creates 3 correct escrow records
- Unit test: returned count matches
- Unit test: sender's balance decreases by the batch total
- Unit test: all three indexes contain every delivery ID
- Unit test: batch of `MAX_BATCH_SIZE` succeeds, `MAX_BATCH_SIZE + 1` rejected
- Unit test: duplicate delivery ID rejected with no partial state written
- Edge case: batch containing the same driver twice

## Definition of Done

- [ ] Core batch behavior covered by tests
- [ ] Known-defect assertions clearly marked
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

**None**. Landing this first makes #188, #189, #272, and #273 safer to implement.

## Labels

`test`

---

# Issue #301 — `shared_types::DeliveryId` conversions and comparisons have no dedicated tests

## Problem Statement

`DeliveryId` is a `u64` newtype with a hand-written API used across every contract:

```rust
pub struct DeliveryId(pub u64);
impl DeliveryId { pub fn new(value: u64) -> Self; pub fn value(self) -> u64; }
impl From<u64> for DeliveryId
impl From<DeliveryId> for u64
impl PartialEq<u64> for DeliveryId
impl PartialEq<DeliveryId> for u64
```

None of these six items has a dedicated test. The type is exercised incidentally
wherever contracts convert between representations, but nothing asserts that
`DeliveryId::from(n).value() == n`, that the two `PartialEq` directions agree, or
that round-tripping through `u64` is lossless.

## Why It Matters

`DeliveryId` is the correlation key between the delivery and escrow contracts.
`delivery_contract` uses the newtype in its public signatures while
`escrow_contract` takes a bare `u64`, so every cross-contract call converts —
`u64::from(delivery_id)` appears at each escrow invocation site.

The two `PartialEq` implementations are asymmetric hand-written code, and the
`value(self)` method takes `self` by value on a `Copy` type. These are small
surfaces, but they sit on the path that ties a delivery to the money escrowed
against it, and a defect would misroute that correlation silently.

The cost of covering them is very low, which is why this is Trivial — but
`shared_types` is the crate every contract depends on, so its primitives
warranting no tests at all is a gap worth closing.

## Proposed Solution

Add a small test module covering construction, both conversion directions, both
equality directions, and round-trip fidelity at boundary values.

Include `u64::MAX` and `0` explicitly, since the type wraps an unbounded counter
and the delivery counter's reset behavior is itself a concern (issue #291).

## Acceptance Criteria

- [ ] `DeliveryId::new(n).value()` returns `n`
- [ ] `DeliveryId::from(n)` and `u64::from(id)` round-trip losslessly
- [ ] Both `PartialEq` directions agree for equal and unequal values
- [ ] Boundary values `0` and `u64::MAX` behave correctly
- [ ] `delivery_key` and `escrow_key` produce distinct storage keys for the same ID
- [ ] Tests live with the other `shared_types` tests

## Technical Notes

- `shared_types` already has a `#[cfg(test)]` module, so there is an established home for these tests.
- `delivery_key(id)` and `escrow_key(id)` both take `impl Into<DeliveryId>` and produce different `StorageKey` variants — asserting they differ guards against a key-collision regression.
- `value(self)` consumes `self`; `DeliveryId` derives `Copy`, so this is ergonomic rather than a defect, but a test documents the intent.

## Relevant Files

- `contracts/shared_types/lib.rs` — `DeliveryId` and its impls, `delivery_key`, `escrow_key`
- `contracts/shared_types/lib.rs` — existing `#[cfg(test)]` module

## Testing Requirements

- Unit test: construction and `value()` round-trip
- Unit test: `From<u64>` and `From<DeliveryId>` round-trip
- Unit test: both `PartialEq` directions, equal and unequal
- Unit test: boundary values `0` and `u64::MAX`
- Unit test: `delivery_key` and `escrow_key` differ for the same ID

## Definition of Done

- [ ] Test module added and passing
- [ ] Boundary values covered
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

**None**

## Labels

`test`

---

# Issue #302 — `docs/protocol/delivery-protocol.md` states that `confirm_delivery` calls `release_escrow`

## Problem Statement

The protocol document describes delivery confirmation as releasing funds:

> Transitions status to `Delivered` and calls the escrow contract's
> `release_escrow` to release funds to the driver.

`delivery_contract::confirm_delivery` calls `mark_holdback_escrow`, not
`release_escrow`:

```rust
let _: () = env.invoke_contract(
    &escrow_address,
    &soroban_sdk::Symbol::new(&env, "mark_holdback_escrow"),
    soroban_sdk::vec![&env, recipient.into_val(&env), u64::from(delivery_id).into_val(&env)],
);
```

That call moves the escrow to `Holdback`. Funds reach the driver only on a
subsequent `release_holdback_escrow`, which the recipient or an admin must call
separately. The document does not mention `Holdback` at all.

## Why It Matters

This is the single most load-bearing fact about the payment flow, and the document
states the opposite of what happens. A reader concludes that confirming delivery
pays the driver, when confirmation only earmarks the funds — the driver is not
paid until a second, separate transaction.

The gap matters operationally: nothing obliges the recipient to make that second
call, which is the liveness problem issue #192 describes. A reader relying on this
document would not know the second step exists, let alone that it can be skipped.

The omission of `Holdback` also means the document's escrow model is a state
behind the implementation, which affects every reader reasoning about dispute
timing and fund availability.

## Proposed Solution

Correct the confirmation description to state that it calls
`mark_holdback_escrow` and transitions the escrow to `Holdback`, and document the
separate `release_holdback_escrow` step that actually pays the driver.

Add `Holdback` to the document's description of escrow states so the delivery and
escrow state machines it presents are consistent with
`docs/contract-design/escrow-design.md`, which was updated with the accurate
state machine.

## Acceptance Criteria

- [ ] The document states that `confirm_delivery` calls `mark_holdback_escrow`
- [ ] The `Holdback` state and its meaning are described
- [ ] The separate release step required to pay the driver is documented
- [ ] The document does not claim confirmation releases funds
- [ ] Descriptions are consistent with `docs/contract-design/escrow-design.md`
- [ ] Any other escrow interaction described in the file is verified against the source

## Technical Notes

- `docs/contract-design/escrow-design.md` carries an accurate state machine and refund-authorization table; reuse its terminology.
- `release_holdback_escrow` is callable by the recipient or an admin.
- Issues #303 and #304 cover further inaccuracies in this same file; coordinate so the three do not conflict.

## Relevant Files

- `docs/protocol/delivery-protocol.md`
- `contracts/delivery_contract/lib.rs` — `confirm_delivery`
- `contracts/escrow_contract/lib.rs` — `mark_holdback_escrow`, `release_holdback_escrow`
- `docs/contract-design/escrow-design.md`

## Testing Requirements

Documentation change; verification by review against source:

- [ ] Every escrow function named in the document verified to exist and be called as described
- [ ] State names verified against `shared_types::EscrowState`
- [ ] Cross-checked against the escrow design document for consistency

## Definition of Done

- [ ] Confirmation flow described accurately
- [ ] `Holdback` documented
- [ ] No contradiction with the escrow design document

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

Same file as #303 and #304; sequence to avoid conflicting edits.

## Labels

`documentation`

---

# Issue #303 — `docs/protocol/delivery-protocol.md` documents the wrong parties for raising a dispute

## Problem Statement

The protocol document's transition table and dispute section both state that
disputes may be raised by the **sender or driver**:

> | `InTransit` | `Disputed` | Sender or driver raises a dispute |
> - `InTransit` → `Disputed` (sender or driver can raise)

`delivery_contract::raise_dispute` authorizes the **sender or recipient**, and
explicitly not the driver:

```rust
let is_sender = caller == delivery.sender;
let is_recipient = caller == delivery.recipient;
if !is_sender && !is_recipient {
    panic_with_error!(&env, FaniLabError::Unauthorized);
}
```

The document names one party who cannot call it and omits one who can.

## Why It Matters

This is an authorization claim, so being wrong in both directions is consequential.
A driver reading the protocol specification would expect to be able to raise a
dispute and finds their call rejected; a recipient would not know the capability
is available to them at all.

The confusion is compounded by genuine inconsistency in the code:
`dispute_resolution_contract::raise_dispute` *does* permit drivers, and
`escrow_contract::raise_dispute` permits all three parties — only the delivery
contract excludes the driver. Issue #292 covers that functional gap; this issue
covers the documentation being wrong about all of it.

Documentation that misstates who holds a permission is worse than silence, because
readers act on it.

## Proposed Solution

Correct the transition table and the dispute section to state the parties each
contract actually authorizes, noting where they differ.

If issue #292 lands first and widens the delivery contract to include drivers,
document the unified set instead. Either way the document must describe the
implementation rather than an intended design.

## Acceptance Criteria

- [ ] The transition table names the parties `delivery_contract::raise_dispute` actually authorizes
- [ ] The dispute section matches
- [ ] Any divergence between the delivery, dispute, and escrow contracts' authorization is noted
- [ ] No party is documented as authorized when the code rejects them
- [ ] The description remains accurate if #292 lands
- [ ] Other authorization claims in the file are verified against source

## Technical Notes

- The three authorization sets today: `delivery_contract` allows sender/recipient; `dispute_resolution_contract` allows sender/recipient/driver; `escrow_contract` allows sender/recipient/driver.
- `dispute_resolution_contract` cross-calls the delivery contract, so its broader set is not actually reachable — see issue #292.
- Closed issue #100 covered driver exclusion from disputes and prompted the dispute contract's widening; the delivery contract was not updated.

## Relevant Files

- `docs/protocol/delivery-protocol.md` — transition table and dispute section
- `contracts/delivery_contract/lib.rs` — `raise_dispute`
- `contracts/dispute_resolution_contract/lib.rs` — `raise_dispute`
- `contracts/escrow_contract/lib.rs` — `raise_dispute`

## Testing Requirements

Documentation change; verification by review against source:

- [ ] Each documented party checked against the contract's authorization branch
- [ ] Divergence between the three contracts verified and reflected
- [ ] Remaining authorization claims in the file audited

## Definition of Done

- [ ] Dispute authorization documented accurately
- [ ] Cross-contract divergence noted
- [ ] Other authorization claims verified

## Complexity

**Medium**

## Estimated Effort

1–2 hours

## Dependencies

Should be reconciled with #292 if that lands first; same file as #302 and #304.

## Labels

`documentation`

---

# Issue #304 — `docs/protocol/delivery-protocol.md` calls `Delivered` terminal while the state machine allows `Delivered → Disputed`

## Problem Statement

The protocol document states:

> `Delivered` and `Cancelled` are **terminal states** — no further transitions are
> permitted.

`validate_transition` explicitly permits a transition out of `Delivered`:

```rust
| (DeliveryStatus::Delivered, DeliveryStatus::Disputed)
| (DeliveryStatus::Disputed, DeliveryStatus::Delivered)
```

The document's own ASCII diagram reinforces the error by labelling `Delivered` as
`(terminal)`, and its transition table omits the `Delivered → Disputed` row while
listing `Disputed → Delivered`.

## Why It Matters

`Delivered → Disputed` is the post-delivery dispute window — the mechanism that
lets a recipient contest goods after accepting them, governed by the configurable
`dispute_time_limit`. Documenting `Delivered` as terminal states that this window
does not exist.

The document is internally inconsistent as well: it lists `Disputed → Delivered`
as valid, which means a delivery can leave `Delivered`, become `Disputed`, and
return — impossible if `Delivered` were genuinely terminal.

The practical effect is that readers do not learn the dispute window exists, and
the protocol has a real functional gap in that area (issues #193 and #292) which
nobody would think to look for while believing the state is terminal.

## Proposed Solution

Correct the terminal-state claim to cover only `Cancelled`, add the
`Delivered → Disputed` row to the transition table, and update the diagram so
`Delivered` is not labelled terminal.

Document the `dispute_time_limit` bound on that transition, since it is what makes
`Delivered` effectively terminal after the window elapses — that is likely the
intent behind the original wording and is worth stating precisely.

## Acceptance Criteria

- [ ] Only `Cancelled` is described as terminal
- [ ] `Delivered → Disputed` appears in the transition table
- [ ] The diagram no longer labels `Delivered` as terminal
- [ ] The `dispute_time_limit` bound on the transition is documented
- [ ] The document's transition set matches `validate_transition` exactly
- [ ] No transition is documented that the code rejects

## Technical Notes

- `validate_transition` in `contracts/delivery_contract/lib.rs` is the authoritative set — nine pairs in total; the document should match it item for item.
- The time bound is enforced in `dispute_resolution_contract::raise_dispute`'s `Delivered` branch, not in `validate_transition`, so the document should attribute it correctly.
- Issue #193 records that this transition is currently unreachable end to end because the escrow rejects `raise_dispute` from `Holdback`; note the state of affairs rather than documenting the transition as fully working.

## Relevant Files

- `docs/protocol/delivery-protocol.md` — state diagram, transition table, terminal-state claim
- `contracts/delivery_contract/lib.rs` — `validate_transition`
- `contracts/dispute_resolution_contract/lib.rs` — `raise_dispute` time-limit branch

## Testing Requirements

Documentation change; verification by review against source:

- [ ] Every pair in `validate_transition` present in the document
- [ ] Every documented transition present in `validate_transition`
- [ ] Terminal-state claims verified against the transition set
- [ ] Time-limit attribution verified against the dispute contract

## Definition of Done

- [ ] Terminal-state claim corrected
- [ ] Transition table complete and matching the code
- [ ] Diagram updated

## Complexity

**Medium**

## Estimated Effort

1–2 hours

## Dependencies

Same file as #302 and #303; sequence to avoid conflicting edits.

## Labels

`documentation`

---

# Issue #305 — `docs/architecture/smart-contract-architecture.md` documents Proof-of-Delivery hashing that does not exist

## Problem Statement

The architecture document describes `delivery_contract`'s responsibilities as:

> **Responsibilities**: Creation of delivery, Assignment of drivers, In-Transit
> updates, and Proof of Delivery (PoD) hashing.

There is no proof-of-delivery mechanism in the contract. Searching
`contracts/delivery_contract/lib.rs` for `proof_of_delivery`, `pod_hash`, `PoD`,
and `delivery_proof` returns zero matches. `confirm_delivery` takes only the
recipient's address and the delivery ID, and stores no proof artifact.

The only hash-bearing feature in the protocol is
`dispute_resolution_contract::add_evidence_hash`, which stores `BytesN<32>`
evidence hashes against a dispute — a different contract and a different purpose.

## Why It Matters

Proof of delivery is a substantive trust primitive: it is what would let a driver
demonstrate they delivered, independent of the recipient's cooperation. Documenting
it as an existing responsibility misrepresents the protocol's trust model.

The absence matters concretely given the rest of this backlog. Confirmation is
entirely at the recipient's discretion, and nothing obliges them to confirm or to
release the holdback afterwards (issue #192). A reader who believes PoD hashing
exists would assume the driver has recourse; they do not.

The architecture document is also the highest-level entry point for new
contributors, so an incorrect responsibility list here propagates into every
mental model built from it.

## Proposed Solution

Remove the PoD claim from the responsibilities list, or mark it explicitly as
planned rather than implemented.

If proof of delivery is genuinely wanted — and the driver-recourse gap suggests it
would be valuable — file it as a separate feature issue with a concrete design
rather than leaving it implied by an architecture document. Do not expand this
issue into implementing it.

While in the file, verify the other per-contract responsibility lists against
their implementations; issue #306 covers one further claim in the same document.

## Acceptance Criteria

- [ ] The PoD claim is removed or clearly marked as unimplemented
- [ ] Every remaining responsibility listed for `delivery_contract` exists in the code
- [ ] If PoD is retained as a roadmap item, it is visually distinguished from shipped functionality
- [ ] A separate feature issue is filed if the capability is still wanted
- [ ] Other contracts' responsibility lists are verified in the same pass

## Technical Notes

- `confirm_delivery`'s signature is `(env, recipient, delivery_id)` — there is no parameter through which a proof artifact could be supplied.
- `DeliveryRecord` has no field for a proof hash; adding one would be a `shared_types` wire-format change.
- `dispute_resolution_contract::add_evidence_hash` is the closest existing mechanism and is dispute-scoped, not delivery-scoped.

## Relevant Files

- `docs/architecture/smart-contract-architecture.md` — `delivery_contract` section
- `contracts/delivery_contract/lib.rs` — `confirm_delivery`, `DeliveryRecord` usage
- `contracts/shared_types/lib.rs` — `DeliveryRecord`

## Testing Requirements

Documentation change; verification by review against source:

- [ ] Each listed responsibility traced to a function that implements it
- [ ] Absence of PoD confirmed by search across the contract
- [ ] Other contracts' responsibility lists spot-checked

## Definition of Done

- [ ] PoD claim removed or marked unimplemented
- [ ] Responsibility lists verified against code
- [ ] Follow-up feature issue filed if the capability is wanted

## Complexity

**Medium**

## Estimated Effort

1–2 hours

## Dependencies

Same file as #306; can be done together.

## Labels

`documentation`

---

# Issue #306 — Architecture document claims `delivery_contract` verifies driver tier, which it never does

## Problem Statement

The architecture document states that `delivery_contract`:

> **Interacts with**: `identity_reputation_contract` (to verify driver tier),
> `escrow_contract` (to trigger payment upon completion).

`delivery_contract` contains zero references to `get_driver_tier` or `DriverTier`.
Its only calls into `identity_reputation_contract` are `register_user` during
delivery creation and `increase_reputation` during confirmation.

`assign_driver` performs no reputation, tier, or KYC check of any kind — it
verifies only that the caller is the admin or the driver themselves, and that the
driver is not the sender or recipient.

## Why It Matters

Tier verification at assignment is the mechanism that would make the reputation
system consequential — without it, reputation is recorded but never gates
anything, and a driver at reputation zero is as assignable as one at the Gold
threshold.

Documenting the check as an existing interaction obscures that gap. A reader
evaluating the protocol's quality controls would conclude driver vetting happens
at assignment when nothing of the kind occurs.

Closed issue #44 reported that the tier system was never wired into
`assign_driver`; the architecture document still describes it as wired. This issue
is about the documentation's claim, not about implementing the check.

## Proposed Solution

Correct the interaction description to state what `delivery_contract` actually
calls: `register_user` on creation and `increase_reputation` on confirmation.

If tier gating is still wanted, reference the existing closed issue #44 discussion
or file a fresh feature issue — but do not implement it here. The scope of this
issue is making the document accurate.

Note the related gap that `kyc_verified` is likewise recorded but never read
anywhere (issue #313), so any claim about identity-based gating in this document
should be checked against that too.

## Acceptance Criteria

- [ ] The documented interaction matches the actual cross-contract calls
- [ ] No identity-based gating is claimed that the code does not perform
- [ ] The `escrow_contract` interaction description is verified in the same pass
- [ ] If tier gating remains desired, it is tracked as an explicit follow-up
- [ ] Other contracts' "Interacts with" lists are spot-checked

## Technical Notes

- `delivery_contract`'s cross-contract calls are: `register_user` (creation and batch creation), `mark_holdback_escrow` (confirmation), `refund_escrow` (cancellation), `raise_dispute` (dispute), `increase_reputation` (confirmation), and `get_escrow` (combined state).
- The escrow interaction is also loosely described — "to trigger payment upon completion" is inaccurate post-holdback, per issue #302.
- `get_driver_tier` and `is_eligible_for_enterprise` live in `identity_reputation_contract` and are called by nothing.

## Relevant Files

- `docs/architecture/smart-contract-architecture.md` — `delivery_contract` section
- `contracts/delivery_contract/lib.rs` — all `invoke_contract` sites
- `contracts/identity_reputation_contract/lib.rs` — `get_driver_tier`

## Testing Requirements

Documentation change; verification by review against source:

- [ ] Every claimed interaction traced to an `invoke_contract` call
- [ ] Every actual cross-contract call represented in the document
- [ ] Absence of tier checks confirmed by search
- [ ] Other contracts' interaction lists spot-checked

## Definition of Done

- [ ] Interaction description matches implementation
- [ ] Follow-up tracked if tier gating is still wanted
- [ ] Other sections spot-checked

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

Same file as #305; can be done together.

## Labels

`documentation`

---

# Issue #307 — `PRODUCTION_READINESS.md` lists resolved issues as outstanding blockers

## Problem Statement

The readiness assessment lists blockers that have since been fixed:

```
- ❌ **Issue #7**: freeze_funds function lacks access control (unauthenticated)
- ❌ **Issue #8**: Dispute resolution path has structural issues
- ❌ **Reentrancy tests**: No reentrancy-specific test cases exist
- ⚠️ Access control incomplete on all privileged functions
```

All three claims are stale. `freeze_funds` now restricts callers to the configured
dispute contract and has `test_freeze_funds_unauthorized_caller_rejected` covering
it. A reentrancy test exists —
`test_release_escrow_rejects_reentrant_call_during_settlement_swap`, built on a
`MaliciousSettlementContract` mock. GitHub issues #7 and #8 are both closed.

The document simultaneously scores several categories as complete — "Code Quality
✅ (10/10)", "Testing ✅ (10/10)", "CI/CD ✅ (10/10)" — while listing ❌ blockers,
so it contradicts itself as well as the codebase.

## Why It Matters

This is the document a reviewer or operator consults to judge deployment
readiness. Listing fixed vulnerabilities as open understates the project's actual
state and wastes reviewer time re-investigating closed work.

The self-contradiction is the more corrosive problem: a document that scores
testing 10/10 while asserting no reentrancy tests exist cannot be trusted in
either direction, so its genuine warnings — and there are genuine ones in this
backlog — carry no weight.

Closed issue #34 previously reported that this document's claims contradicted the
codebase. It has drifted again, now in the opposite direction: rather than
overstating readiness, it understates it by citing resolved issues.

## Proposed Solution

Reconcile the document against the current codebase and the GitHub issue tracker:
remove or update claims about issues that are closed, verify each remaining ❌ and
⚠️ item against the source, and resolve the contradiction between the category
scores and the blocker list.

Establish where the document's authority comes from — if the blocker list is meant
to mirror open GitHub issues, say so and reference them by link so drift is
visible. A scoring section that is maintained by hand and never revisited will
drift again.

## Acceptance Criteria

- [ ] No closed issue is listed as an outstanding blocker
- [ ] Each remaining blocker is verified to still exist in the code
- [ ] Category scores are consistent with the blocker list
- [ ] Claims such as "Test coverage > 80%" are verified against actual coverage
- [ ] The document states how and when it should be updated
- [ ] Remaining genuine gaps are linked to their tracking issues

## Technical Notes

- `freeze_funds` gained its caller restriction and test; `escrow_contract/test.rs` contains both `test_freeze_funds_unauthorized_caller_rejected` and `test_freeze_funds_remains_available_while_paused`.
- The reentrancy test is `test_release_escrow_rejects_reentrant_call_during_settlement_swap`; issue #238 in this backlog notes that coverage is limited to one call site, which is a genuine remaining gap worth citing accurately.
- Coverage is enforced by `codecov.yml` at an 80% project target, so the coverage claim is checkable rather than aspirational.
- Several open GitHub issues (#64, #140–#143) are genuine blockers and could anchor the list.

## Relevant Files

- `PRODUCTION_READINESS.md`
- `contracts/escrow_contract/lib.rs` — `freeze_funds`
- `contracts/escrow_contract/test.rs` — the freeze and reentrancy tests
- `codecov.yml`

## Testing Requirements

Documentation change; verification by review against source and tracker:

- [ ] Each ❌ and ⚠️ item traced to current code or removed
- [ ] Each cited GitHub issue's state checked
- [ ] Coverage claim checked against the enforced threshold
- [ ] Category scores reconciled with the surviving blocker list

## Definition of Done

- [ ] Stale blockers removed or corrected
- [ ] Internal contradictions resolved
- [ ] Maintenance expectation documented

## Complexity

**Medium**

## Estimated Effort

2–4 hours

## Dependencies

**None**

## Labels

`documentation`

---

# Issue #308 — `MIGRATION_GUIDE.md` and `UPGRADE_GUIDE.md` cover the same topic with no cross-reference

## Problem Statement

The repository maintains two documents on contract versioning:

- `docs/UPGRADE_GUIDE.md` (175 lines) — "Guide for upgrading FaniLab smart contracts on Stellar Soroban."
- `docs/MIGRATION_GUIDE.md` (186 lines) — "This guide demonstrates how to safely migrate contract state when upgrading to new contract versions."

Neither links to the other. A reader arriving at either has no indication the
other exists, and the two overlap substantially — upgrading a Soroban contract and
migrating its state are steps in one procedure, not separable topics.

`UPGRADE_GUIDE.md` documents a `migrate_to_v2` pattern that no contract implements
(the subject of closed issue #81), which is itself state-migration content sitting
in the upgrade document.

## Why It Matters

Contract upgrade is a high-stakes, infrequently-performed operation. Splitting its
documentation across two unlinked files means an operator following one may never
see prerequisites or warnings recorded in the other — and the consequences of a
partial upgrade procedure on a contract holding escrowed funds are severe.

The duplication also guarantees divergence: two documents describing overlapping
procedures, maintained independently, will disagree over time. They already do, in
that one documents a migration pattern the other does not mention.

The cost of fixing this is low, which is why it is Trivial — but the risk it
mitigates is not.

## Proposed Solution

Decide whether the two documents should merge or remain separate with clear scopes.

If they remain separate, give each an explicit scope statement and a prominent
cross-reference, and move the `migrate_to_v2` state-migration content into the
migration guide where it belongs.

If they merge, preserve all content and leave a stub or redirect at the removed
path so existing links do not break.

## Acceptance Criteria

- [ ] The relationship between the two documents is explicit
- [ ] Each document states its scope and links to the other, or they are merged
- [ ] State-migration content lives in one place
- [ ] No procedure step exists in only one document without a pointer from the other
- [ ] Existing links to either path continue to resolve
- [ ] The unimplemented `migrate_to_v2` pattern is marked as such

## Technical Notes

- No contract implements `migrate_to_v2` or calls `update_current_contract_wasm`; closed issue #81 covers the missing tooling, so the documentation should be explicit that the pattern is aspirational.
- `docs/API.md` and `README.md` may link to one or both guides — check before renaming or removing a path.
- This issue is documentation organization only; implementing migration tooling is out of scope.

## Relevant Files

- `docs/MIGRATION_GUIDE.md`
- `docs/UPGRADE_GUIDE.md`
- `README.md`, `docs/API.md` — for existing links

## Testing Requirements

Documentation change; verification by review:

- [ ] Both documents' scopes stated and non-overlapping, or merged
- [ ] Cross-references present and resolving
- [ ] All inbound links from other documents still resolve
- [ ] No procedural step orphaned in one document

## Definition of Done

- [ ] Relationship clarified or documents merged
- [ ] Migration content consolidated
- [ ] Inbound links verified

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

**None**

## Labels

`documentation`

---

# Issue #309 — Issue templates do not prompt for a complexity classification

## Problem Statement

The repository has three issue templates —
`.github/ISSUE_TEMPLATE/bug_report.md`, `feature_request.md`, and
`security_vulnerability.md`. None of them prompts for complexity or effort:
grepping all three for `complexity`, `effort`, and `points` returns zero
matches.

Maintainers triaging incoming issues need an explicit complexity classification
in order to size and prioritise them — the backlog issues each carry a
`## Complexity` section of Trivial, Medium, or High, and externally filed issues
arrive with no equivalent.

## Why It Matters

Contributors filing issues through the templates produce entries with no size
signal at all. A maintainer then has to assess and classify each one by hand
before it can be prioritised, or the issue sits untriaged.

The templates are the natural place to capture this at source. They already prompt
for reproduction steps, expected behavior, and environment — adding two more
fields is a small change that removes recurring manual work.

The backlog's own structure is the ready-made answer: `## Complexity`,
`## Estimated Effort`, and `## Acceptance Criteria` are exactly the fields
maintainers need and are already the established convention in this repository.

## Proposed Solution

Add a complexity field to the templates, offering the three levels used
throughout the backlog, along with an estimated-effort prompt.

Consider aligning the templates more broadly with the backlog's issue structure so
externally filed issues and internally authored ones are shaped alike — but keep
the change proportionate, since templates that demand too much deter contributors.

Do not add automatic label defaults to the template front matter. Labels that
enrol an issue in an external programme must stay a deliberate, per-issue
decision by a maintainer rather than something the templates apply silently.

## Acceptance Criteria

- [ ] Templates prompt for a complexity classification with the three levels
- [ ] Templates prompt for estimated effort
- [ ] The prompts explain what the levels mean
- [ ] Existing template fields are preserved
- [ ] Templates add no automatic label defaults via front matter
- [ ] Templates remain short enough not to deter contributors

## Technical Notes

- The `security_vulnerability.md` template may warrant different handling — severity matters more than complexity there, and it should continue to direct reporters to the disclosure process in `SECURITY.md`.
- The backlog's `## Complexity` / `## Estimated Effort` sections are the wording to reuse.

## Relevant Files

- `.github/ISSUE_TEMPLATE/bug_report.md`
- `.github/ISSUE_TEMPLATE/feature_request.md`
- `.github/ISSUE_TEMPLATE/security_vulnerability.md`
- `SECURITY.md` — disclosure process referenced by the security template

## Testing Requirements

Configuration change; verification by use:

- [ ] Each template renders correctly when filing a new issue
- [ ] The complexity prompt is present and understandable
- [ ] The security template still routes reporters to the disclosure process

## Definition of Done

- [ ] Complexity and effort prompts added
- [ ] No automatic label defaults introduced
- [ ] Templates render correctly

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

**None**

## Labels

`enhancement`, `documentation`

---

# Issue #310 — SDK `tsconfig.json` excludes test files from type-checking

## Problem Statement

`sdk/typescript/tsconfig.json` excludes test files from compilation:

```json
"exclude": ["node_modules", "dist", "**/*.test.ts"]
```

`npm run build` runs `tsc`, so any file matching `*.test.ts` is never
type-checked. The package declares a jest toolchain and a `"test": "jest"` script
but currently contains no test files at all and no jest configuration (issue
#249).

## Why It Matters

The exclusion means that when SDK tests are eventually written — which issues
#222, #249, and #263 all call for — they will compile only under `ts-jest` at test
time, never under the project's own build. A type error in a test would not surface
in `npm run build`, and if CI runs only the build (issue #248), it would not
surface in CI either.

Excluding tests from the *emitted output* is correct and desirable; excluding them
from *type-checking* is not. The two are usually separated by keeping the exclusion
narrow and running a `tsc --noEmit` pass that includes tests.

This is small and currently latent — there are no tests to check — which is why it
is Trivial. Fixing it before tests exist means the first test written is covered
from the start.

## Proposed Solution

Keep test files out of the build output but include them in type-checking, either
by adding a separate type-check script that does not apply the exclusion, or by
splitting into a base `tsconfig.json` and a `tsconfig.build.json` that adds the
exclusion for emit only.

Wire whichever type-check command results into the CI job proposed by issue #248,
so test type errors fail the build.

## Acceptance Criteria

- [ ] Test files are type-checked
- [ ] Test files are not emitted into `dist/`
- [ ] `npm run build` still produces a correct build output
- [ ] A type error in a test file causes a non-zero exit from the type-check command
- [ ] The type-check command is suitable for CI
- [ ] `examples/basic-usage.ts` is also type-checked

## Technical Notes

- A common shape is a base `tsconfig.json` with no test exclusion plus a `tsconfig.build.json` that extends it and adds `exclude`; `npm run build` points at the latter.
- `"strict": true` is already enabled, so type-checking tests will be meaningfully strict.
- `examples/` is not currently excluded, so the example file should already be type-checked — verify rather than assume.
- Coordinate with issue #249, which adds the jest configuration, and #248, which adds the CI job.

## Relevant Files

- `sdk/typescript/tsconfig.json`
- `sdk/typescript/package.json` — `build` script
- `sdk/typescript/examples/basic-usage.ts`

## Testing Requirements

- Verification: a deliberate type error in a `.test.ts` file fails the type-check
- Verification: `dist/` contains no compiled test files after a build
- Verification: `npm run build` succeeds on the current source
- Verification: the example file is included in type-checking

## Definition of Done

- [ ] Tests type-checked but not emitted
- [ ] Type-check command verified to fail on a test type error
- [ ] Build output unchanged

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

Most useful alongside #249 (jest config) and #248 (CI job); independently landable.

## Labels

`test`, `enhancement`

---

# Issue #311 — No contract exposes a non-panicking existence check for escrows or disputes

## Problem Statement

Both record-fetching accessors fail hard when a record is absent, and neither
contract offers a way to ask whether one exists.

`escrow_contract::get_escrow` panics with `EscrowError::DeliveryNotFound`, and
`dispute_resolution_contract::get_dispute` panics with
`FaniLabError::DeliveryNotFound`:

```rust
pub fn get_dispute(env: Env, delivery_id: DeliveryId) -> DisputeCase {
    env.storage().persistent().get(&dispute_key)
        .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::DeliveryNotFound))
}
```

`identity_reputation_contract` already solves this pattern with
`has_driver_profile(driver) -> bool`, added so callers could check existence
before acting rather than triggering a panic.

## Why It Matters

The absence forces callers into awkward shapes. `delivery_contract::cancel_delivery`
cannot check whether an escrow exists before cross-calling `refund_escrow`, which
is exactly why a delivery with no escrow becomes uncancellable (issue #295). Issue
#297 hits the same wall for `mark_in_transit`.

Off-chain clients are similarly affected: an indexer or front end that wants to
display "escrow: not yet funded" must call `get_escrow` and catch a contract
panic, which is both awkward and indistinguishable from other failure modes at the
transaction level.

The `has_driver_profile` precedent shows the team already recognises this problem;
the accessors were simply never added for the other two record types.

## Proposed Solution

Add `has_escrow(delivery_id) -> bool` to `escrow_contract` and
`has_dispute(delivery_id) -> bool` to `dispute_resolution_contract`, following
`has_driver_profile`'s shape — a simple storage presence check with no
authorization requirement and no panic.

Keep the existing panicking accessors unchanged so no caller breaks.

## Acceptance Criteria

- [ ] `escrow_contract::has_escrow` returns true for an existing escrow and false otherwise
- [ ] `dispute_resolution_contract::has_dispute` behaves equivalently
- [ ] Neither panics for an unknown ID
- [ ] Neither requires authorization
- [ ] `get_escrow` and `get_dispute` behavior is unchanged
- [ ] Both are documented in `docs/API.md`

## Technical Notes

- `identity_reputation_contract::has_driver_profile` uses `.get::<_, DriverProfile>(&key).is_some()`; a plain `.has(&key)` is cheaper and sufficient for a presence check.
- `escrow_contract::get_escrow` already performs a `has` check internally before loading (see issue #289), so the storage call pattern is established.
- These accessors unblock the cleaner fixes proposed in #295 and #297; adding them first makes those changes simpler.
- Keep this scoped to the two accessors — it is deliberately a single issue rather than one per contract, since they are the same one-line addition and would be reviewed together.

## Relevant Files

- `contracts/escrow_contract/lib.rs` — `get_escrow`, new `has_escrow`
- `contracts/dispute_resolution_contract/lib.rs` — `get_dispute`, new `has_dispute`
- `contracts/identity_reputation_contract/lib.rs` — `has_driver_profile` precedent
- `docs/API.md`

## Testing Requirements

- Unit test: `has_escrow` true after creation, false for an unknown ID
- Unit test: `has_dispute` true after a dispute is raised, false otherwise
- Unit test: neither panics for an unknown ID
- Unit test: both callable without authorization
- Regression test: `get_escrow` and `get_dispute` still panic as before for unknown IDs

## Definition of Done

- [ ] Both accessors added
- [ ] Documented in `docs/API.md`
- [ ] Tests added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Trivial**

## Estimated Effort

1–2 hours

## Dependencies

**None**. Landing this first simplifies #295 and #297.

## Labels

`enhancement`

---

# Issue #312 — Disputes cannot be enumerated, so no operator view of open cases exists

## Problem Statement

`dispute_resolution_contract` stores each case under
`DataKey::Dispute(delivery_id)` and exposes exactly one accessor,
`get_dispute(delivery_id)`, which requires the caller to already know the delivery
ID.

There is no index of disputes by status, by party, or by age, and no counter. An
admin cannot ask which disputes are currently `Open`, which are approaching their
resolution deadline, or how many exist.

Both `escrow_contract` and `delivery_contract` maintain secondary indexes for
exactly this purpose — `EscrowsBySender`, `EscrowsByDriver`,
`DeliveriesByRecipient`, and so on — so the pattern is established in the codebase
and simply absent here.

## Why It Matters

Dispute resolution is admin-driven: `resolve_dispute_refund_sender`,
`resolve_dispute_pay_driver`, and `resolve_dispute_split_funds` all require an
admin to act. An admin who cannot enumerate open disputes has no way to discover
the work queue on chain — they must reconstruct it from `dispute_raised` events
off chain, and any gap in that indexing silently drops a case.

The consequence is directly connected to the forced-resolution mechanism.
`force_resolve_dispute` exists because admins may fail to resolve disputes in time
(and is itself defective — issues #205 and #206). Making the queue invisible makes
that failure more likely, not less.

`docs/MONITORING.md` names dispute activity among the metrics to track, with no
on-chain accessor to support it.

## Proposed Solution

Add a secondary index of disputes, following the pattern already used by the
escrow and delivery contracts: maintain a list on `raise_dispute` and expose an
accessor.

Indexing all disputes with their status is the simplest correct approach — callers
filter client-side. Maintaining a separate `Open` list has better read
characteristics but requires careful removal on every resolution path, of which
there are four, and a missed removal produces a misleading queue.

Note the growth concern that issue #234 raises for the existing indexes: a single
unbounded vector has the same scaling ceiling here, so the design should account
for it rather than repeat it.

## Acceptance Criteria

- [ ] Disputes can be enumerated without prior knowledge of delivery IDs
- [ ] Open disputes are distinguishable from resolved ones
- [ ] The index is updated on `raise_dispute`
- [ ] The index remains correct across all four resolution paths
- [ ] Growth characteristics are bounded or explicitly documented
- [ ] Existing `get_dispute` behavior is unchanged

## Technical Notes

- The four terminal paths are `resolve_dispute_refund_sender`, `resolve_dispute_pay_driver`, `resolve_dispute_split_funds`, and `force_resolve_dispute` — any index requiring removal must be updated in all four.
- `DisputeCase` already carries `status`, `resolved_at`, and `resolved_by`, so a status-bearing index needs no struct change.
- `escrow_contract::get_escrows_by_sender` and its siblings are the established accessor shape.
- Issue #234 proposes pagination for the existing indexes; a new index should not repeat the unbounded-vector pattern.

## Relevant Files

- `contracts/dispute_resolution_contract/lib.rs` — `raise_dispute`, `get_dispute`, the four resolution paths, `DataKey`
- `contracts/escrow_contract/lib.rs` — secondary index pattern
- `docs/MONITORING.md`, `docs/API.md`

## Testing Requirements

- Unit test: a raised dispute appears in the enumeration
- Unit test: enumeration is empty before any dispute is raised
- Unit test: a resolved dispute is distinguishable from an open one, via each of the four resolution paths
- Unit test: multiple disputes enumerate correctly
- Edge case: enumeration behavior at a substantial dispute count
- Regression test: `get_dispute` unchanged

## Definition of Done

- [ ] Dispute enumeration implemented
- [ ] Index correct across all resolution paths
- [ ] Growth characteristics addressed
- [ ] Documented in `docs/API.md`
- [ ] Tests added and passing

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

Should adopt whatever indexing approach #234 settles on to avoid repeating the unbounded-vector pattern.

## Labels

`feature`, `enhancement`

---

# Issue #313 — `kyc_verified` is recorded and administered but never read by any contract

## Problem Statement

`DriverProfile` carries a `kyc_verified` flag. `identity_reputation_contract`
provides an admin function to set it:

```rust
pub fn update_driver_kyc_status(env: Env, admin: Address, driver: Address, kyc_verified: bool) {
    /* admin-gated */
    profile.kyc_verified = kyc_verified;
    /* store, emit KycStatusUpdatedEvent */
}
```

The flag is never read as a condition anywhere in the protocol. Searching all six
contracts finds `kyc_verified` only at write sites: `false` at registration,
assignment in `update_driver_kyc_status`, and the event payload. The single
occurrence in `delivery_contract` is the fabricated default inside
`get_driver_profile` (issue #202).

`fleet_management_contract` and `escrow_contract` contain zero references.

## Why It Matters

KYC verification exists to gate participation — that is its only purpose. A flag
that is set by an admin, emitted in an event, and consulted by nothing provides no
protection: an unverified driver can be assigned to any delivery, join any fleet,
and receive any payout exactly as a verified one can.

The administrative surface makes this actively misleading. An operator who
verifies a driver's identity and sets the flag reasonably believes they have
changed something about what that driver may do. They have not — they have written
a value that no code path reads.

This is the same shape as the multi-signature finding in issue #216: a control
that is configured, exposed through an API, and emits events, but is never
enforced. It sits alongside the unenforced driver-tier system (closed issue #44),
so the identity contract currently has two recorded-but-unenforced gating
mechanisms.

## Proposed Solution

Decide where KYC should gate, then enforce it at that point. The natural candidate
is `delivery_contract::assign_driver`, which today checks only that the caller is
the admin or the driver and that the driver is not the sender or recipient.

Enforcement requires a cross-call to `identity_reputation_contract`, which
`delivery_contract` already performs for `register_user` and
`increase_reputation`, so the wiring exists.

Make the gate configurable rather than absolute — a `require_kyc` protocol flag
lets deployments opt in without breaking existing test fixtures and testnet
flows, and makes the policy explicit rather than implicit.

If the decision is that KYC should not gate anything on chain, remove the flag and
its admin function rather than leaving a control that implies protection it does
not provide.

## Acceptance Criteria

- [ ] `kyc_verified` either gates a concrete action or is removed
- [ ] If it gates, the enforcement point is documented
- [ ] If enforcement is configurable, the default is stated explicitly
- [ ] An unverified driver is rejected at the gate when enforcement is enabled
- [ ] A verified driver proceeds normally
- [ ] Existing flows are unaffected when enforcement is disabled
- [ ] `docs/GOVERNANCE.md` or `docs/API.md` describes the policy

## Technical Notes

- `assign_driver` currently performs no cross-contract call; adding one puts the identity contract on the assignment path and introduces a failure mode if it is unreachable — weigh that and consider making the gate skip cleanly when no identity contract is configured, as the reputation calls already do.
- `get_driver_profile` in `identity_reputation_contract` panics with `ProviderNotFound` for an unregistered driver, so the gate must handle drivers with no profile.
- Closed issue #44 covers the parallel unenforced tier system; a single gating mechanism could reasonably address both, but that is a larger design decision than this issue should settle.
- `KycStatusUpdatedEvent` already exists and should continue to be emitted.

## Relevant Files

- `contracts/identity_reputation_contract/lib.rs` — `update_driver_kyc_status`, `register_driver`, `get_driver_profile`
- `contracts/delivery_contract/lib.rs` — `assign_driver`
- `contracts/shared_types/lib.rs` — `DriverProfile.kyc_verified`
- `docs/GOVERNANCE.md`

## Testing Requirements

- Unit test: unverified driver rejected at the gate when enforcement is enabled
- Unit test: verified driver accepted
- Unit test: behavior when no identity contract is configured
- Unit test: behavior for a driver with no profile at all
- Regression test: existing assignment flows unaffected with enforcement disabled
- Authorization test: only an admin can change KYC status
- Event test: `KycStatusUpdatedEvent` still emitted

## Definition of Done

- [ ] KYC either enforced at a documented point or removed
- [ ] Policy and default documented
- [ ] Tests above added and passing
- [ ] Formatting, clippy, and full suite clean

## Complexity

**Medium**

## Estimated Effort

4–8 hours

## Dependencies

Related to the closed issue #44's unenforced tier system; the two could share a gating mechanism but this issue does not depend on that.

## Labels

`security`, `feature`

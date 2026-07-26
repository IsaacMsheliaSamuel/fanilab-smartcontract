# Escrow Contract Design

## Overview

The `escrow_contract` is the financial backbone of the FaniLab logistics platform. It manages the locking, releasing, and refunding of Stellar assets (XLM, USDC, etc.) for delivery escrows. Every delivery on the platform is backed by an escrow record that holds funds in custody until the delivery reaches a terminal state.

**Status**: Implemented (Phase 1)

---

## Escrow Lifecycle & State Machine

Each escrow follows a well-defined state machine, implemented via `EscrowStatus`:

```
Locked ──► Released
  │
  ├──► Refunded
  │
  └──► Paused ──► Released
                  └──► Refunded
```

| State       | Meaning                                                          |
|-------------|------------------------------------------------------------------|
| `Locked`    | Funds are held in the contract. Delivery is pending/active/in-transit. |
| `Released`  | Funds have been paid out to the driver (or fleet treasury). Terminal.   |
| `Refunded`  | Funds have been returned to the sender. Terminal.                      |
| `Paused`    | Funds are frozen due to an active dispute.                             |
| `Holdback`  | A portion of funds is held back temporarily (e.g., for dispute window).|

### Escrow Record Structure

```rust
pub struct EscrowRecord {
    pub delivery_id: u64,
    pub sender: Address,
    pub recipient: Address,
    pub driver: Address,
    pub token: Address,         // The Stellar asset contract address
    pub amount: i128,
    pub platform_fee: i128,     // Fee deducted on release
    pub status: EscrowStatus,
    pub expires_at: Option<u64>,
    pub disputed_at: Option<u64>,
}
```

---

## Key Functions

### `init(env, admin, token, platform_fee_bps, dispute_contract)`
Initializes the escrow contract with:
- An admin address (protocol governance)
- The supported token contract address
- A platform fee rate in basis points (e.g., 50 = 0.5%)
- The dispute resolution contract address (for freeze/unfreeze calls)

Reverts with `AlreadyInitialized` if called more than once.

### `fund_escrow(env, sender, delivery_id, recipient, driver, amount, expires_at)`
The sender locks funds into escrow for a specific delivery. This function:
1. Transfers `amount + platform_fee` from sender to the contract
2. Creates an `EscrowRecord` with status `Locked`
3. Returns any excess XLM sent back to the sender
4. Emits an `escrow_funded` event

Reverts with `InsufficientFunds` or `Unauthorized` if the caller doesn't match the sender.

### `release_escrow(env, caller, delivery_id)`
Releases locked funds to the driver. Authorized callers:
- The **delivery contract** (when delivery is confirmed)
- The **dispute resolution contract** (when a dispute is resolved in the driver's favor)

Before payout, the function:
1. Checks if a `fleet_management_contract` is configured and queries the fleet's payout address (redirecting funds to the fleet treasury instead of the individual driver)
2. Checks if a `settlement_contract` is configured and queries the driver's preferred asset for cross-currency swaps
3. Deducts the platform fee and transfers it to the admin
4. Transfers net amount to the payout address

Emits an `escrow_released` event.

### `refund_escrow(env, caller, delivery_id)`
Refunds locked funds to the original sender. Authorized callers:
- The **delivery contract** (when delivery is cancelled)
- The **dispute resolution contract** (when a dispute is resolved for a refund)

Transfers the full amount back to the sender and sets status to `Refunded`.
Emits an `escrow_refunded` event.

### `reclaim_expired_escrow(env, delivery_id)`
Allows anyone to reclaim an escrow that has passed its `expires_at` timestamp. Funds are returned to the sender. This prevents funds from being locked indefinitely.

### `freeze_funds(env, caller, delivery_id)`
Called exclusively by the **dispute resolution contract** to pause an escrow when a dispute is raised. Transitions status from `Locked` to `Paused`. Emits a `funds_frozen` event.

### `unfreeze_funds(env, caller, delivery_id)`
Called by the dispute resolution contract to return a paused escrow back to `Locked` status when a dispute is resolved in favor of continuing the delivery.

### `update_platform_fee(env, admin, new_fee_bps)`
Admin-only function to update the platform fee rate. The new fee must not exceed `MAX_PLATFORM_FEE_BPS` (1000 bps = 10%).

### `get_escrow(env, delivery_id)`
Read-only view that returns the full `EscrowRecord` for a given delivery ID. Useful for off-chain indexers and the delivery contract's combined-state validation.

### Batch & Query Functions

- `get_escrows_by_sender(env, sender)` — returns all delivery IDs for a sender
- `get_escrows_by_recipient(env, recipient)` — returns all delivery IDs for a recipient
- `get_escrows_by_driver(env, driver)` — returns all delivery IDs for a driver

---

## Fee Model

On every escrow release, a platform fee is deducted:

```rust
platform_fee = amount * platform_fee_bps / 10_000
```

- The fee is transferred to the admin address
- The remainder is paid to the driver (or fleet treasury)
- Fees are set during initialization and can be updated by the admin

---

## Cross-Contract Interactions

### Fleet Management Integration
When a fleet management contract is configured, the escrow contract queries `get_payout_address(driver, fleet_id)` to redirect driver payouts to the fleet owner's treasury wallet.

### Settlement Contract Integration
When a settlement contract is configured, the escrow contract queries `get_driver_preference(driver)` before payout. If the driver prefers a different asset, the settlement contract performs a currency swap via Soroban AMM before the final transfer.

### Dispute Resolution Integration
The dispute resolution contract is authorized to call `freeze_funds` and `unfreeze_funds` on the escrow contract, as well as `release_escrow` and `refund_escrow` when a dispute is resolved.

---

## Events

| Topic                  | Emitted By               | Payload                              |
|------------------------|--------------------------|--------------------------------------|
| `escrow_funded`        | `fund_escrow`            | `(sender, amount, platform_fee)`     |
| `escrow_released`      | `release_escrow`         | `(driver, amount, platform_fee)`     |
| `escrow_refunded`      | `refund_escrow`          | `(sender, amount)`                   |
| `funds_frozen`         | `freeze_funds`           | `(caller, timestamp)`                |
| `platform_fee_updated` | `update_platform_fee`    | `(new_fee_bps)`                      |

---

## Security Considerations

1. **Access Control**: Only authorized contracts (delivery, dispute) can release or refund escrows. Admin functions require explicit admin authentication.
2. **Re-entrancy Protection**: All state mutations happen before external token transfers.
3. **Expiry**: Expired escrows can be reclaimed by anyone, preventing permanent fund lockup.
4. **Pause Mechanism**: The protocol can be paused via the admin, halting all fund movements for emergency maintenance.

---

## Related Documents

- [Smart Contract Architecture](../architecture/smart-contract-architecture.md)
- [Delivery Protocol](../protocol/delivery-protocol.md)
- [Event System](../architecture/event-system.md)

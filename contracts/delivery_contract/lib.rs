#![no_std]
#![allow(deprecated)] // events().publish() is deprecated in SDK 27.0.0 but still functional

use shared_types::FaniLabError;
use shared_types::{
    delivery_key, events, DeliveryCreatedEvent, DeliveryConfirmedEvent, DeliveryDisputedEvent,
    DriverAssignedEvent, DeliveryMetadata, DriverProfile, StorageKey,
};
pub use shared_types::{DeliveryId, DeliveryRecord, DeliveryStatus};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, Address, Env, Symbol,
};

// Local DeliveryMetadata removed in favor of shared_types::DeliveryMetadata

/// Maximum deliveries per batch to stay within Soroban resource limits.
pub const MAX_BATCH_SIZE: u32 = 100;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    DeliveryCounter,
    EscrowContract,
    /// Secondary index: deliveries created by sender (Vec<DeliveryId>).
    DeliveriesBySender(Address),
    /// Secondary index: deliveries with recipient (Vec<DeliveryId>).
    DeliveriesByRecipient(Address),
    IdentityReputationContract,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum DeliveryError {
    InvalidState = 1,
    InvalidMetadata = 2,
}

mod constants {
    pub const MAX_LOCATION_LEN: u32 = 256;
    pub const MAX_WEIGHT_GRAMS: u32 = 1_000_000;
}

/// Validate whether a status transition is permitted by the delivery state machine.
///
/// Allowed transitions:
///   Pending   → Active, Cancelled
///   Active    → InTransit, Disputed, Cancelled
///   InTransit → Delivered, Disputed
///   Disputed  → Delivered, Cancelled
///   Delivered, Cancelled → (terminal, no transitions)
pub fn validate_transition(from: DeliveryStatus, to: DeliveryStatus) -> Result<(), FaniLabError> {
    let valid = match (from, to) {
        (DeliveryStatus::Pending, DeliveryStatus::Active) => true,
        (DeliveryStatus::Pending, DeliveryStatus::Cancelled) => true,
        (DeliveryStatus::Active, DeliveryStatus::InTransit) => true,
        (DeliveryStatus::Active, DeliveryStatus::Disputed) => true,
        (DeliveryStatus::Active, DeliveryStatus::Cancelled) => true,
        (DeliveryStatus::InTransit, DeliveryStatus::Delivered) => true,
        (DeliveryStatus::InTransit, DeliveryStatus::Disputed) => true,
        (DeliveryStatus::Disputed, DeliveryStatus::Delivered) => true,
        (DeliveryStatus::Disputed, DeliveryStatus::Cancelled) => true,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(FaniLabError::InvalidState)
    }
}

fn validate_delivery_metadata(env: &Env, metadata: &DeliveryMetadata) -> Result<(), DeliveryError> {
    if metadata.origin.len() > constants::MAX_LOCATION_LEN {
        return Err(DeliveryError::InvalidMetadata);
    }
    if metadata.destination.len() > constants::MAX_LOCATION_LEN {
        return Err(DeliveryError::InvalidMetadata);
    }
    if metadata.cargo_description.weight_grams > constants::MAX_WEIGHT_GRAMS {
        return Err(DeliveryError::InvalidMetadata);
    }
    Ok(())
}

#[contract]
pub struct DeliveryContract;

#[contractimpl]
impl DeliveryContract {
    pub fn init(env: Env, admin: Address, escrow_contract: Address) {
        if env.storage().instance().has(&StorageKey::Admin) {
            panic_with_error!(&env, FaniLabError::AlreadyInitialized);
        }
        env.storage().instance().set(&StorageKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::EscrowContract, &escrow_contract);
        env.storage()
            .persistent()
            .set(&DataKey::DeliveryCounter, &0u64);

        env.events().publish(
            (Symbol::new(&env, "DeliveryContractInitialized"),),
            (admin, escrow_contract),
        );
    }

    pub fn set_identity_reputation_contract(
        env: Env,
        admin: Address,
        identity_contract: Address,
    ) {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::NotInitialized));
        if admin != stored_admin {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }
        env.storage()
            .instance()
            .set(&DataKey::IdentityReputationContract, &identity_contract);
    }

    pub fn get_identity_reputation_contract(env: Env) -> Option<Address> {
        env.storage()
            .instance()
            .get(&DataKey::IdentityReputationContract)
    }

    pub fn create_delivery(
        env: Env,
        sender: Address,
        recipient: Address,
        metadata: DeliveryMetadata,
    ) -> DeliveryId {
        sender.require_auth();

        validate_delivery_metadata(&env, &metadata)
            .unwrap_or_else(|_| panic_with_error!(&env, DeliveryError::InvalidMetadata));

        let mut counter: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::DeliveryCounter)
            .unwrap_or(0);
        counter += 1;
        env.storage()
            .persistent()
            .set(&DataKey::DeliveryCounter, &counter);

        let delivery_id = DeliveryId::from(counter);

        let record = DeliveryRecord {
            delivery_id,
            sender: sender.clone(),
            recipient: recipient.clone(),
            driver: None,
            status: DeliveryStatus::Pending,
            metadata,
            created_at: env.ledger().timestamp(),
            delivered_at: None,
            transit_started_at: None,
        };

        let key = delivery_key(delivery_id);
        env.storage().persistent().set(&key, &record);
        env.storage().persistent().extend_ttl(&key, 518400, 518400);

        // Update secondary indexes.
        let sender_key = DataKey::DeliveriesBySender(sender.clone());
        let mut sender_deliveries: soroban_sdk::Vec<DeliveryId> = env
            .storage()
            .persistent()
            .get(&sender_key)
            .unwrap_or_else(|| soroban_sdk::Vec::new(&env));
        sender_deliveries.push_back(delivery_id);
        env.storage()
            .persistent()
            .set(&sender_key, &sender_deliveries);
        env.storage().persistent().extend_ttl(&sender_key, 518400, 518400);

        let recipient_key = DataKey::DeliveriesByRecipient(recipient.clone());
        let mut recipient_deliveries: soroban_sdk::Vec<DeliveryId> = env
            .storage()
            .persistent()
            .get(&recipient_key)
            .unwrap_or_else(|| soroban_sdk::Vec::new(&env));
        recipient_deliveries.push_back(delivery_id);
        env.storage()
            .persistent()
            .set(&recipient_key, &recipient_deliveries);
        env.storage().persistent().extend_ttl(&recipient_key, 518400, 518400);

        env.events().publish(
            (events::delivery_created(&env),),
            DeliveryCreatedEvent {
                delivery_id: delivery_id.value(),
                sender,
                amount: 0,
            },
        );

        delivery_id
    }

    /// Create multiple deliveries in a single transaction.  Sender must authorize.
    /// Returns Vec of created delivery IDs.  Each delivery funds escrow individually
    /// via cross-contract calls to the escrow contract.
    pub fn create_deliveries_batch(
        env: Env,
        sender: Address,
        recipient: Address,
        metadata_list: soroban_sdk::Vec<DeliveryMetadata>,
    ) -> soroban_sdk::Vec<DeliveryId> {
        sender.require_auth();

        if metadata_list.len() > MAX_BATCH_SIZE as u32 {
            panic!("BatchTooLarge");
        }

        let mut result = soroban_sdk::Vec::new(&env);
        let mut counter: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::DeliveryCounter)
            .unwrap_or(0);

        let timestamp = env.ledger().timestamp();

        // Pre-load sender and recipient indexes for efficient batch updates.
        let sender_key = DataKey::DeliveriesBySender(sender.clone());
        let mut sender_deliveries: soroban_sdk::Vec<DeliveryId> = env
            .storage()
            .persistent()
            .get(&sender_key)
            .unwrap_or_else(|| soroban_sdk::Vec::new(&env));

        let recipient_key = DataKey::DeliveriesByRecipient(recipient.clone());
        let mut recipient_deliveries: soroban_sdk::Vec<DeliveryId> = env
            .storage()
            .persistent()
            .get(&recipient_key)
            .unwrap_or_else(|| soroban_sdk::Vec::new(&env));

        for i in 0..metadata_list.len() {
            if let Some(metadata) = metadata_list.get(i) {
                counter += 1;
                env.storage()
                    .persistent()
                    .set(&DataKey::DeliveryCounter, &counter);

                let delivery_id = DeliveryId::from(counter);

                let record = DeliveryRecord {
                    delivery_id,
                    sender: sender.clone(),
                    recipient: recipient.clone(),
                    driver: None,
                    status: DeliveryStatus::Pending,
                    metadata,
                    created_at: timestamp,
                    delivered_at: None,
                    transit_started_at: None,
                };

                let key = delivery_key(delivery_id);
                env.storage().persistent().set(&key, &record);
                env.storage().persistent().extend_ttl(&key, 518400, 518400);

                sender_deliveries.push_back(delivery_id);
                recipient_deliveries.push_back(delivery_id);

                env.events().publish(
                    (soroban_sdk::Symbol::new(&env, "delivery_created"),),
                    (delivery_id, sender.clone()),
                );

                result.push_back(delivery_id);
            }
        }

        // Save updated indexes.
        env.storage()
            .persistent()
            .set(&sender_key, &sender_deliveries);
        env.storage().persistent().extend_ttl(&sender_key, 518400, 518400);

        env.storage()
            .persistent()
            .set(&recipient_key, &recipient_deliveries);
        env.storage().persistent().extend_ttl(&recipient_key, 518400, 518400);

        result
    }

    pub fn cancel_delivery(env: Env, sender: Address, delivery_id: DeliveryId) {
        sender.require_auth();

        let key = delivery_key(delivery_id);
        let mut delivery: DeliveryRecord = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::DeliveryNotFound));

        if delivery.sender != sender {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }

        validate_transition(delivery.status.clone(), DeliveryStatus::Cancelled)
            .unwrap_or_else(|_| panic_with_error!(&env, FaniLabError::InvalidState));

        let escrow_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::EscrowContract)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::NotInitialized));

        use soroban_sdk::IntoVal;
        let _: () = env.invoke_contract(
            &escrow_address,
            &soroban_sdk::Symbol::new(&env, "refund_escrow"),
            soroban_sdk::vec![
                &env,
                sender.into_val(&env),
                u64::from(delivery_id).into_val(&env),
            ],
        );

        delivery.status = DeliveryStatus::Cancelled;
        env.storage().persistent().set(&key, &delivery);
        env.storage().persistent().extend_ttl(&key, 518400, 518400);

        env.events().publish(
            (events::delivery_cancelled(&env),),
            (delivery_id, sender),
        );
    }

    pub fn assign_driver(env: Env, caller: Address, delivery_id: DeliveryId, driver: Address) {
        caller.require_auth();

        let is_admin = Self::is_admin(&env, &caller);
        let is_self_assignment = caller == driver;

        if !is_admin && !is_self_assignment {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }

        let key = delivery_key(delivery_id);
        let mut delivery: DeliveryRecord = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::DeliveryNotFound));

        if driver == delivery.sender || driver == delivery.recipient {
            panic!("InvalidDriver");
        }

        validate_transition(delivery.status.clone(), DeliveryStatus::Active)
            .unwrap_or_else(|_| panic_with_error!(&env, FaniLabError::InvalidState));

        delivery.driver = Some(driver.clone());
        delivery.status = DeliveryStatus::Active;

        env.storage().persistent().set(&key, &delivery);
        env.storage().persistent().extend_ttl(&key, 518400, 518400);

        env.events().publish(
            (events::driver_assigned(&env),),
            DriverAssignedEvent {
                delivery_id: delivery_id.value(),
                driver,
            },
        );
    }

    /// Allow the assigned driver to mark a delivery as actively in transit.
    /// Transitions: Active → InTransit. Records the ledger timestamp.
    pub fn mark_in_transit(env: Env, driver: Address, delivery_id: DeliveryId) {
        driver.require_auth();

        let key = delivery_key(delivery_id);
        let mut delivery: DeliveryRecord = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::DeliveryNotFound));

        // Verify caller is the assigned driver for this delivery
        match &delivery.driver {
            Some(assigned) if *assigned == driver => {}
            _ => panic_with_error!(&env, FaniLabError::Unauthorized),
        }

        validate_transition(delivery.status.clone(), DeliveryStatus::InTransit)
            .unwrap_or_else(|_| panic_with_error!(&env, FaniLabError::InvalidState));

        let timestamp = env.ledger().timestamp();
        delivery.status = DeliveryStatus::InTransit;
        delivery.transit_started_at = Some(timestamp);

        env.storage().persistent().set(&key, &delivery);
        env.storage().persistent().extend_ttl(&key, 518400, 518400);

        env.events().publish(
            (events::delivery_in_transit(&env),),
            (delivery_id, driver, timestamp),
        );
    }

    pub fn confirm_delivery(env: Env, recipient: Address, delivery_id: DeliveryId) {
        recipient.require_auth();

        let key = delivery_key(delivery_id);
        let mut delivery: DeliveryRecord = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::DeliveryNotFound));

        if recipient != delivery.recipient {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }

        if let Some(driver) = &delivery.driver {
            if driver == &recipient || driver == &delivery.sender {
                panic!("InvalidDelivery");
            }
        }

        validate_transition(delivery.status.clone(), DeliveryStatus::Delivered)
            .unwrap_or_else(|_| panic_with_error!(&env, FaniLabError::InvalidState));

        let escrow_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::EscrowContract)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::NotInitialized));

        use soroban_sdk::IntoVal;
        let _: () = env.invoke_contract(
            &escrow_address,
            &soroban_sdk::Symbol::new(&env, "mark_holdback_escrow"),
            soroban_sdk::vec![
                &env,
                recipient.into_val(&env),
                u64::from(delivery_id).into_val(&env),
            ],
        );

        delivery.status = DeliveryStatus::Delivered;
        delivery.delivered_at = Some(env.ledger().timestamp());

        env.storage().persistent().set(&key, &delivery);
        env.storage().persistent().extend_ttl(&key, 518400, 518400);

        if let Some(driver_addr) = &delivery.driver {
            if let Some(identity_contract) = Self::get_identity_reputation_contract(env.clone()) {
                let cargo_desc = &delivery.metadata.cargo_description;
                let _: () = env.invoke_contract(
                    &identity_contract,
                    &Symbol::new(&env, "increase_reputation"),
                    soroban_sdk::vec![
                        &env,
                        env.current_contract_address().into_val(&env),
                        driver_addr.into_val(&env),
                        u64::from(delivery_id).into_val(&env),
                        cargo_desc.weight_grams.into_val(&env),
                        cargo_desc.fragile.into_val(&env),
                    ],
                );
            }
        }

        env.events().publish(
            (events::delivery_confirmed(&env),),
            DeliveryConfirmedEvent {
                delivery_id: delivery_id.value(),
                recipient,
                timestamp: delivery.delivered_at.unwrap_or(0),
            },
        );
    }

    /// Allow sender or recipient to escalate a delivery to Disputed and pause
    /// the escrow via a cross-contract call. The escrow call executes first so
    /// that delivery state is never mutated when the escrow call fails.
    pub fn raise_dispute(env: Env, caller: Address, delivery_id: DeliveryId) {
        caller.require_auth();

        let key = delivery_key(delivery_id);
        let mut delivery: DeliveryRecord = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::DeliveryNotFound));

        let is_sender = caller == delivery.sender;
        let is_recipient = caller == delivery.recipient;
        if !is_sender && !is_recipient {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }

        validate_transition(delivery.status.clone(), DeliveryStatus::Disputed)
            .unwrap_or_else(|_| panic_with_error!(&env, FaniLabError::InvalidState));

        let escrow_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::EscrowContract)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::NotInitialized));

        // Cross-contract call first: if escrow raises dispute fails, delivery
        // state is not mutated (implicit rollback via propagated panic).
        use soroban_sdk::IntoVal;
        let _: () = env.invoke_contract(
            &escrow_address,
            &Symbol::new(&env, "raise_dispute"),
            soroban_sdk::vec![
                &env,
                caller.into_val(&env),
                u64::from(delivery_id).into_val(&env),
            ],
        );

        let timestamp = env.ledger().timestamp();
        delivery.status = DeliveryStatus::Disputed;

        env.storage().persistent().set(&key, &delivery);
        env.storage().persistent().extend_ttl(&key, 518400, 518400);

        env.events().publish(
            (events::delivery_disputed(&env),),
            DeliveryDisputedEvent {
                delivery_id: delivery_id.value(),
                reporter: caller,
                timestamp,
            },
        );
    }

    fn is_admin(env: &Env, caller: &Address) -> bool {
        if let Some(admin) = env
            .storage()
            .instance()
            .get::<_, Address>(&StorageKey::Admin)
        {
            admin == *caller
        } else {
            false
        }
    }

    pub fn get_driver_profile(env: Env, driver: Address) -> DriverProfile {
        let driver_key = StorageKey::DriverProfile(driver.clone());
        env.storage()
            .persistent()
            .get(&driver_key)
            .unwrap_or_else(|| DriverProfile {
                address: driver,
                deliveries_completed: 0,
                reputation_score: 0,
                registered_at: env.ledger().timestamp(),
                kyc_verified: false,
            })
    }

    pub fn get_delivery(env: Env, delivery_id: DeliveryId) -> DeliveryRecord {
        let key = delivery_key(delivery_id);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::DeliveryNotFound))
    }

    /// Returns combined delivery and escrow state, and flags known-invalid combinations.
    /// Validates that delivery and escrow states are synchronized according to protocol invariants.
    pub fn get_combined_state(
        env: Env,
        delivery_id: DeliveryId,
    ) -> (DeliveryRecord, shared_types::EscrowRecord, bool) {
        let delivery = Self::get_delivery(env.clone(), delivery_id);

        let escrow_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::EscrowContract)
            .unwrap_or_else(|| panic!("EscrowNotConfigured"));

        use soroban_sdk::IntoVal;
        let escrow: shared_types::EscrowRecord = env.invoke_contract(
            &escrow_address,
            &Symbol::new(&env, "get_escrow"),
            soroban_sdk::vec![&env, u64::from(delivery_id).into_val(&env)],
        );

        let is_synchronized = Self::validate_state_sync(&delivery, &escrow);
        (delivery, escrow, is_synchronized)
    }

    /// Validates that delivery and escrow states match expected protocol invariants.
    /// Returns true if states are synchronized, false if a mismatch is detected.
    fn validate_state_sync(
        delivery: &DeliveryRecord,
        escrow: &shared_types::EscrowRecord,
    ) -> bool {
        match (&delivery.status, &escrow.status) {
            // Pending/Active: escrow should be Locked
            (DeliveryStatus::Pending, shared_types::EscrowStatus::Locked) => true,
            (DeliveryStatus::Active, shared_types::EscrowStatus::Locked) => true,

            // InTransit: escrow should still be Locked
            (DeliveryStatus::InTransit, shared_types::EscrowStatus::Locked) => true,

            // Delivered: escrow must be Released (funds moved to driver)
            (DeliveryStatus::Delivered, shared_types::EscrowStatus::Released) => true,

            // Disputed: escrow must be Paused
            (DeliveryStatus::Disputed, shared_types::EscrowStatus::Paused) => true,

            // Cancelled: escrow should be Refunded
            (DeliveryStatus::Cancelled, shared_types::EscrowStatus::Refunded) => true,

            // Any other combination is a mismatch
            _ => false,
        }
    }

    /// Get all delivery IDs created by a sender.
    pub fn get_deliveries_by_sender(env: Env, sender: Address) -> soroban_sdk::Vec<DeliveryId> {
        let key = DataKey::DeliveriesBySender(sender);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| soroban_sdk::Vec::new(&env))
    }

    /// Get all delivery IDs with a specific recipient.
    pub fn get_deliveries_by_recipient(env: Env, recipient: Address) -> soroban_sdk::Vec<DeliveryId> {
        let key = DataKey::DeliveriesByRecipient(recipient);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| soroban_sdk::Vec::new(&env))
    }
}

#[cfg(test)]
mod test;
// TTL management - implementation in progress

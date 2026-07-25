#![no_std]
#![allow(deprecated)] // events().publish() is deprecated in SDK 27.0.0 but still functional

use shared_types::{
    escrow_key, events, EscrowRecord, EscrowStatus, FaniLabError, ProtocolConfig, StorageKey,
    escrow_key, events, ttl, DeliveryStatus, EscrowRecord, EscrowStatus, FaniLabError,
    ProtocolConfig, StorageKey,
};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, token, Address, Env,
    IntoVal, Symbol,
};

pub mod constants {
    pub const PROTOCOL_VERSION: u32 = 1;
    pub const MAX_BATCH_SIZE: u32 = 100;
    pub const DEFAULT_ESCROW_EXPIRY_SECONDS: u64 = 30 * 24 * 60 * 60; // 30 days
    pub const MAX_PLATFORM_FEE_BPS: u32 = 1000;
}

fn require_admin(env: &Env, caller: &Address) {
    let stored_admin: Address = env
        .storage()
        .instance()
        .get(&StorageKey::Admin)
        .unwrap_or_else(|| panic_with_error!(env, FaniLabError::NotInitialized));
    if *caller != stored_admin {
        panic_with_error!(env, FaniLabError::Unauthorized);
    }
}

fn is_admin(env: &Env, caller: &Address) -> bool {
    let stored_admin: Address = env
        .storage()
        .instance()
        .get(&StorageKey::Admin)
        .expect("Not initialized");
    *caller == stored_admin
}

fn is_protocol_paused(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}

fn require_not_paused(env: &Env) {
    if is_protocol_paused(env) {
        panic_with_error!(env, FaniLabError::ProtocolPaused);
    }
}

fn load_protocol_config(env: &Env) -> ProtocolConfig {
    env.storage()
        .instance()
        .get(&StorageKey::ProtocolConfig)
        .unwrap_or_else(|| panic_with_error!(env, FaniLabError::NotInitialized))
}

fn save_protocol_config(env: &Env, config: &ProtocolConfig) {
    env.storage()
        .instance()
        .set(&StorageKey::ProtocolConfig, config);
}

fn calculate_fee(amount: i128, platform_fee_bps: u32) -> i128 {
    amount.saturating_mul(platform_fee_bps as i128) / 10_000
}

fn get_settlement_contract(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::SettlementContract)
}

fn get_fleet_management_contract(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::FleetManagementContract)
}

fn payout_driver(
    env: &Env,
    token: &Address,
    driver: &Address,
    amount: i128,
    fleet_management_addr: Option<&Address>,
    fleet_id: Option<u64>,
) {
    if amount <= 0 {
        return;
    }

    let mut payout_address = driver.clone();

    if let (Some(fleet_addr), Some(fid)) = (fleet_management_addr, fleet_id) {
        let treasury: Address = env.invoke_contract(
            fleet_addr,
            &Symbol::new(env, "get_payout_address"),
            soroban_sdk::vec![env, driver.into_val(env), fid.into_val(env)],
        );
        payout_address = treasury;
    }

    if let Some(settlement_addr) = get_settlement_contract(env) {
        let preferred_asset: Option<Address> = env.invoke_contract(
            &settlement_addr,
            &Symbol::new(env, "get_driver_preference"),
            soroban_sdk::vec![env, driver.into_val(env)],
        );

        if let Some(preferred_asset) = preferred_asset {
            if preferred_asset != token.clone() {
                let slippage_tolerance_bps: u32 = load_protocol_config(env).slippage_tolerance_bps;
                let min_amount_out = amount.saturating_mul(10000 - slippage_tolerance_bps as i128) / 10000;
                let _: () = env.invoke_contract(
                    &settlement_addr,
                    &Symbol::new(env, "execute_settlement_swap"),
                    soroban_sdk::vec![
                        env,
                        env.current_contract_address().into_val(env),
                        token.into_val(env),
                        preferred_asset.into_val(env),
                        payout_address.into_val(env),
                        amount.into_val(env),
                        min_amount_out.into_val(env),
                    ],
                );
                return;
            }
        }
    }

    token::Client::new(env, token).transfer(&env.current_contract_address(), &payout_address, &amount);
}

fn save_escrow(env: &Env, delivery_id: u64, record: &EscrowRecord) {
    let key = escrow_key(delivery_id);
    env.storage().persistent().set(&key, record);
    env.storage().persistent().extend_ttl(
        &key,
        ttl::LEDGER_TTL_THRESHOLD,
        ttl::LEDGER_TTL_EXTEND_TO,
    );
}

fn load_escrow(env: &Env, delivery_id: u64) -> EscrowRecord {
    let key = escrow_key(delivery_id);
    let record: EscrowRecord = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| panic_with_error!(env, EscrowError::DeliveryNotFound));
    env.storage().persistent().extend_ttl(
        &key,
        ttl::LEDGER_TTL_THRESHOLD,
        ttl::LEDGER_TTL_EXTEND_TO,
    );
    record
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    PendingAdmin,
    SettlementContract,
    /// Secondary index: escrows by sender (Vec<u64> delivery IDs).
    EscrowsBySender(Address),
    /// Secondary index: escrows by recipient (Vec<u64> delivery IDs).
    EscrowsByRecipient(Address),
    /// Secondary index: escrows by driver (Vec<u64> delivery IDs).
    EscrowsByDriver(Address),
    Paused,
    FleetManagementContract,
    DisputeResolutionContract,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum EscrowError {
    InvalidState = 1,
    DeliveryNotFound = 2,
    InsufficientFunds = 3,
    DuplicateDelivery = 4,
    InvalidFee = 5,
    InvalidToken = 6,
    InvalidAmount = 6,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeUpdated {
    pub old_fee: u32,
    pub new_fee: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolInitialized {
    pub admin: Address,
    pub token: Address,
    pub platform_fee_bps: u32,
    pub protocol_version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementContractUpdated {
    pub old_address: Option<Address>,
    pub new_address: Address,
}

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    pub fn init(env: Env, admin: Address, token: Address, platform_fee_bps: u32) {
        if env.storage().instance().has(&StorageKey::Admin) {
            panic_with_error!(&env, FaniLabError::AlreadyInitialized);
        }
        if platform_fee_bps > constants::MAX_PLATFORM_FEE_BPS {
            panic_with_error!(&env, EscrowError::InvalidFee);
        }
        env.storage().instance().set(&StorageKey::Admin, &admin);
        save_protocol_config(
            &env,
            &ProtocolConfig {
                token: token.clone(),
                platform_fee_bps,
                protocol_version: constants::PROTOCOL_VERSION,
                slippage_tolerance_bps: 500, // Default 5% slippage tolerance
            },
        );

        env.events().publish(
            (Symbol::new(&env, "ProtocolInitialized"),),
            ProtocolInitialized {
                admin,
                token,
                platform_fee_bps,
                protocol_version: constants::PROTOCOL_VERSION,
            },
        );

        env.storage().instance().extend_ttl(
            ttl::LEDGER_TTL_THRESHOLD,
            ttl::LEDGER_TTL_EXTEND_TO,
        );
    }

    pub fn update_platform_fee(env: Env, admin: Address, new_fee_bps: u32) {
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::NotInitialized));
        if admin != stored_admin {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }
        admin.require_auth();
        if new_fee_bps > constants::MAX_PLATFORM_FEE_BPS {
            panic_with_error!(&env, EscrowError::InvalidFee);
        }
        let mut config = load_protocol_config(&env);
        let old_fee = config.platform_fee_bps;
        config.platform_fee_bps = new_fee_bps;
        save_protocol_config(&env, &config);
        env.events().publish(
            (Symbol::new(&env, "FeeUpdated"),),
            FeeUpdated {
                old_fee,
                new_fee: new_fee_bps,
            },
        );

        env.storage().instance().extend_ttl(
            ttl::LEDGER_TTL_THRESHOLD,
            ttl::LEDGER_TTL_EXTEND_TO,
        );
    }

    pub fn get_platform_fee(env: Env) -> u32 {
        load_protocol_config(&env).platform_fee_bps
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&StorageKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::NotInitialized))
    }

    pub fn get_token(env: Env) -> Address {
        load_protocol_config(&env).token
    }

    pub fn get_protocol_version(env: Env) -> u32 {
        load_protocol_config(&env).protocol_version
    }

    pub fn update_slippage_tolerance(env: Env, admin: Address, new_slippage_bps: u32) {
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::NotInitialized));
        if admin != stored_admin {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }
        admin.require_auth();
        if new_slippage_bps > 10000 {
            panic_with_error!(&env, EscrowError::InvalidFee);
        }
        let mut config = load_protocol_config(&env);
        config.slippage_tolerance_bps = new_slippage_bps;
        save_protocol_config(&env, &config);
    }

    pub fn get_slippage_tolerance(env: Env) -> u32 {
        load_protocol_config(&env).slippage_tolerance_bps
    }

    pub fn set_settlement_contract(env: Env, admin: Address, settlement_contract: Address) {
        admin.require_auth();
        require_admin(&env, &admin);
        let old_address = get_settlement_contract(&env);
        env.storage()
            .instance()
            .set(&DataKey::SettlementContract, &settlement_contract);

        env.storage().instance().extend_ttl(
            ttl::LEDGER_TTL_THRESHOLD,
            ttl::LEDGER_TTL_EXTEND_TO,
        env.events().publish(
            (Symbol::new(&env, "SettlementContractUpdated"),),
            SettlementContractUpdated {
                old_address,
                new_address: settlement_contract,
            },
        );
    }

    pub fn get_settlement_contract(env: Env) -> Option<Address> {
        get_settlement_contract(&env)
    }

    pub fn set_fleet_management_contract(env: Env, admin: Address, fleet_contract: Address) {
    pub fn set_dispute_resolution_contract(
        env: Env,
        admin: Address,
        dispute_contract: Address,
    ) {
        admin.require_auth();
        require_admin(&env, &admin);
        env.storage()
            .instance()
            .set(&DataKey::FleetManagementContract, &fleet_contract);
    }

    pub fn get_fleet_management_contract(env: Env) -> Option<Address> {
        get_fleet_management_contract(&env)
            .set(&DataKey::DisputeResolutionContract, &dispute_contract);
    }

    pub fn get_dispute_resolution_contract(env: Env) -> Option<Address> {
        env.storage()
            .instance()
            .get(&DataKey::DisputeResolutionContract)
    }

    pub fn propose_admin(env: Env, current_admin: Address, new_admin: Address) {
        current_admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::NotInitialized));
        if stored_admin != current_admin {
            panic!("caller is not the admin");
        }
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        env.storage().instance().extend_ttl(
            ttl::LEDGER_TTL_THRESHOLD,
            ttl::LEDGER_TTL_EXTEND_TO,
        );
    }

    pub fn accept_admin(env: Env, new_admin: Address) {
        new_admin.require_auth();
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .expect("no pending admin");
        if pending != new_admin {
            panic!("caller is not the pending admin");
        }
        let old_admin: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::NotInitialized));
        env.storage().instance().set(&StorageKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        env.storage().instance().extend_ttl(
            ttl::LEDGER_TTL_THRESHOLD,
            ttl::LEDGER_TTL_EXTEND_TO,
        );
        env.events().publish(
            (Symbol::new(&env, "AdminTransferred"),),
            (old_admin, new_admin),
        );
    }

    pub fn set_paused(env: Env, admin: Address, paused: bool) {
        admin.require_auth();
        require_admin(&env, &admin);
        env.storage().instance().set(&DataKey::Paused, &paused);
        env.events().publish(
            (Symbol::new(&env, "ProtocolPauseStatusChanged"),),
            (admin, paused),
        );
    }

    pub fn is_paused(env: Env) -> bool {
        is_protocol_paused(&env)
    }

    // ── Escrow lifecycle ──────────────────────────────────────────────────────

    pub fn create_escrow(
        env: Env,
        sender: Address,
        recipient: Address,
        driver: Address,
        delivery_id: u64,
        token: Address,
        amount: i128,
        fleet_id: Option<u64>,
    ) {
        sender.require_auth();
        if amount <= 0 {
            panic_with_error!(&env, EscrowError::InvalidAmount);
        }
        if env.storage().persistent().has(&escrow_key(delivery_id)) {
            panic_with_error!(&env, EscrowError::DuplicateDelivery);
        }
        let config = load_protocol_config(&env);
        if token != config.token {
            panic_with_error!(&env, EscrowError::InvalidToken);
        }
        token::Client::new(&env, &token).transfer(
            &sender,
            &env.current_contract_address(),
            &amount,
        );
        let record_token_clone = token.clone();
        let created_at = env.ledger().timestamp();
        let expires_at = created_at.saturating_add(constants::DEFAULT_ESCROW_EXPIRY_SECONDS);
        save_escrow(
            &env,
            delivery_id,
            &EscrowRecord {
                sender: sender.clone(),
                recipient: recipient.clone(),
                driver: driver.clone(),
                token,
                amount,
                status: EscrowStatus::Locked,
                created_at,
                expires_at: Some(expires_at),
                disputed_by: None,
                disputed_at: None,
                fleet_id,
            },
        );

        // Update secondary indexes.
        let sender_key = DataKey::EscrowsBySender(sender.clone());
        let mut sender_escrows: soroban_sdk::Vec<u64> = env
            .storage()
            .persistent()
            .get(&sender_key)
            .unwrap_or_else(|| soroban_sdk::Vec::new(&env));
        sender_escrows.push_back(delivery_id);
        env.storage()
            .persistent()
            .set(&sender_key, &sender_escrows);
        env.storage().persistent().extend_ttl(
            &sender_key,
            constants::ESCROW_TTL_THRESHOLD,
            constants::ESCROW_TTL_EXTEND_TO,
        );

        let recipient_key = DataKey::EscrowsByRecipient(recipient.clone());
        let mut recipient_escrows: soroban_sdk::Vec<u64> = env
            .storage()
            .persistent()
            .get(&recipient_key)
            .unwrap_or_else(|| soroban_sdk::Vec::new(&env));
        recipient_escrows.push_back(delivery_id);
        env.storage()
            .persistent()
            .set(&recipient_key, &recipient_escrows);
        env.storage().persistent().extend_ttl(
            &recipient_key,
            constants::ESCROW_TTL_THRESHOLD,
            constants::ESCROW_TTL_EXTEND_TO,
        );

        let driver_key = DataKey::EscrowsByDriver(driver.clone());
        let mut driver_escrows: soroban_sdk::Vec<u64> = env
            .storage()
            .persistent()
            .get(&driver_key)
            .unwrap_or_else(|| soroban_sdk::Vec::new(&env));
        driver_escrows.push_back(delivery_id);
        env.storage()
            .persistent()
            .set(&driver_key, &driver_escrows);
        env.storage().persistent().extend_ttl(
            &driver_key,
            constants::ESCROW_TTL_THRESHOLD,
            constants::ESCROW_TTL_EXTEND_TO,
        );

        env.events().publish(
            (events::escrow_funded(&env),),
            shared_types::EscrowFundedEvent {
                delivery_id,
                sender,
                token: record_token_clone,
                amount,
            },
        );
    }

    /// Create multiple escrows in a single transaction.  Sender must authorize.
    /// Takes a list of (delivery_id, driver, amount) tuples. All escrows use the
    /// configured protocol token. Returns the count of escrows created.
    pub fn create_escrows_batch(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        escrow_list: soroban_sdk::Vec<(u64, Address, i128)>,
    ) -> u32 {
        sender.require_auth();

        if escrow_list.len() > constants::MAX_BATCH_SIZE as u32 {
            panic_with_error!(&env, EscrowError::InvalidState);
        }

        // Pre-load indexes for efficient batch updates.
        let sender_key = DataKey::EscrowsBySender(sender.clone());
        let mut sender_escrows: soroban_sdk::Vec<u64> = env
            .storage()
            .persistent()
            .get(&sender_key)
            .unwrap_or_else(|| soroban_sdk::Vec::new(&env));

        let recipient_key = DataKey::EscrowsByRecipient(recipient.clone());
        let mut recipient_escrows: soroban_sdk::Vec<u64> = env
            .storage()
            .persistent()
            .get(&recipient_key)
            .unwrap_or_else(|| soroban_sdk::Vec::new(&env));

        let mut driver_indexes = soroban_sdk::Map::new(&env);

        let mut count = 0u32;
        for i in 0..escrow_list.len() {
            if let Some((delivery_id, driver, amount)) = escrow_list.get(i) {
                if env.storage().persistent().has(&escrow_key(delivery_id)) {
                    panic_with_error!(&env, EscrowError::DuplicateDelivery);
                }
                token::Client::new(&env, &token).transfer(
                    &sender,
                    &env.current_contract_address(),
                    &amount,
                );
                save_escrow(
                    &env,
                    delivery_id,
                    &EscrowRecord {
                        sender: sender.clone(),
                        recipient: recipient.clone(),
                        driver: driver.clone(),
                        token: token.clone(),
                        amount,
                        status: EscrowStatus::Locked,
                        created_at: env.ledger().timestamp(),
                        disputed_by: None,
                        disputed_at: None,
                    },
                );
                sender_escrows.push_back(delivery_id);
                recipient_escrows.push_back(delivery_id);

                // Track drivers separately for efficient index updates.
                let driver_key = DataKey::EscrowsByDriver(driver.clone());
                if !driver_indexes.contains_key(&driver_key) {
                    let existing: soroban_sdk::Vec<u64> = env
                        .storage()
                        .persistent()
                        .get(&driver_key)
                        .unwrap_or_else(|| soroban_sdk::Vec::new(&env));
                    driver_indexes.set(driver_key, existing);
                }
                if let Some(mut driver_escrows_vec) = driver_indexes.get(&driver_key) {
                    driver_escrows_vec.push_back(delivery_id);
                    driver_indexes.set(driver_key, driver_escrows_vec);
                }

                env.events().publish(
                    (events::escrow_funded(&env), delivery_id),
                    (sender.clone(), recipient.clone(), amount),
                );
                count += 1;
            }
        }

        // Save all updated indexes.
        env.storage()
            .persistent()
            .set(&sender_key, &sender_escrows);
        env.storage().persistent().extend_ttl(
            &sender_key,
            constants::ESCROW_TTL_THRESHOLD,
            constants::ESCROW_TTL_EXTEND_TO,
        );

        env.storage()
            .persistent()
            .set(&recipient_key, &recipient_escrows);
        env.storage().persistent().extend_ttl(
            &recipient_key,
            constants::ESCROW_TTL_THRESHOLD,
            constants::ESCROW_TTL_EXTEND_TO,
        );

        for i in 0..driver_indexes.len() {
            if let Some((driver_key, driver_escrows_vec)) = driver_indexes.get_index(i) {
                env.storage()
                    .persistent()
                    .set(&driver_key, &driver_escrows_vec);
                env.storage().persistent().extend_ttl(
                    &driver_key,
                    constants::ESCROW_TTL_THRESHOLD,
                    constants::ESCROW_TTL_EXTEND_TO,
                );
            }
        }

        count
    pub fn mark_holdback_escrow(env: Env, caller: Address, delivery_id: u64) {
        caller.require_auth();
        let mut record = load_escrow(&env, delivery_id);
        let recipient_authorized = caller == record.recipient;
        if !recipient_authorized {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }
        if record.status != EscrowStatus::Locked {
            panic_with_error!(&env, EscrowError::InvalidState);
        }
        record.status = EscrowStatus::Holdback;
        save_escrow(&env, delivery_id, &record);
        env.events().publish(
            (Symbol::new(&env, "escrow_holdback_marked"), delivery_id),
            (caller, env.ledger().timestamp()),
        );
    }

    pub fn release_escrow(env: Env, caller: Address, delivery_id: u64) {
        caller.require_auth();
        require_not_paused(&env);
        let mut record = load_escrow(&env, delivery_id);
        let admin_authorized = is_admin(&env, &caller);
        let recipient_authorized = caller == record.recipient;
        if !admin_authorized && !recipient_authorized {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }
        if record.status != EscrowStatus::Locked {
            panic_with_error!(&env, EscrowError::InvalidState);
        }
        // Balance verification guard: confirm contract holds sufficient funds before transfer
        let contract_balance =
            token::Client::new(&env, &record.token).balance(&env.current_contract_address());
        if contract_balance < record.amount {
            panic_with_error!(&env, EscrowError::InsufficientFunds);
        }
        let platform_fee_bps: u32 = env
            .storage()
            .instance()
            .get::<_, ProtocolConfig>(&StorageKey::ProtocolConfig)
            .map(|config| config.platform_fee_bps)
            .unwrap_or(0);
        let platform_fee = calculate_fee(record.amount, platform_fee_bps);
        let driver_amount = record.amount.saturating_sub(platform_fee);

        let fleet_management = get_fleet_management_contract(&env);
        payout_driver(
            &env,
            &record.token,
            &record.driver,
            driver_amount,
            fleet_management.as_ref(),
            record.fleet_id,
        );

        if platform_fee > 0 {
            let admin: Address = env
                .storage()
                .instance()
                .get(&StorageKey::Admin)
                .expect("Not initialized");
            token::Client::new(&env, &record.token).transfer(
                &env.current_contract_address(),
                &admin,
                &platform_fee,
            );
        }

        record.status = EscrowStatus::Released;
        save_escrow(&env, delivery_id, &record);
        env.events().publish(
            (events::escrow_released(&env),),
            shared_types::EscrowReleasedEvent {
                delivery_id,
                driver: record.driver,
                amount: driver_amount,
                platform_fee,
            },
        );
    }

    pub fn refund_escrow(env: Env, caller: Address, delivery_id: u64) {
        caller.require_auth();
        require_not_paused(&env);
        let mut record = load_escrow(&env, delivery_id);
        let admin_authorized = is_admin(&env, &caller);
        let sender_authorized = caller == record.sender;
        if !admin_authorized && !sender_authorized {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }
        if record.status != EscrowStatus::Locked
            && record.status != EscrowStatus::Paused
            && record.status != EscrowStatus::Holdback
        {
            panic_with_error!(&env, EscrowError::InvalidState);
        }
        // Balance verification guard: confirm contract holds sufficient funds before transfer
        let contract_balance =
            token::Client::new(&env, &record.token).balance(&env.current_contract_address());
        if contract_balance < record.amount {
            panic_with_error!(&env, EscrowError::InsufficientFunds);
        }
        token::Client::new(&env, &record.token).transfer(
            &env.current_contract_address(),
            &record.sender,
            &record.amount,
        );
        record.status = EscrowStatus::Refunded;
        save_escrow(&env, delivery_id, &record);
        env.events().publish(
            (events::escrow_refunded(&env),),
            shared_types::EscrowRefundedEvent {
                delivery_id,
                sender: record.sender,
                amount: record.amount,
            },
        );
    }

    pub fn raise_dispute(env: Env, caller: Address, delivery_id: u64) {
        caller.require_auth();
        let mut record = load_escrow(&env, delivery_id);
        if caller != record.sender && caller != record.recipient {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }
        if record.status != EscrowStatus::Locked {
            panic_with_error!(&env, EscrowError::InvalidState);
        }
        let timestamp = env.ledger().timestamp();
        record.status = EscrowStatus::Paused;
        record.disputed_by = Some(caller.clone());
        record.disputed_at = Some(timestamp);
        save_escrow(&env, delivery_id, &record);
        env.events().publish(
            (events::delivery_disputed(&env),),
            shared_types::DeliveryDisputedEvent {
                delivery_id,
                reporter: caller,
                timestamp,
            },
        );
    }

    pub fn resolve_dispute(env: Env, caller: Address, delivery_id: u64, release_to_driver: bool) {
        caller.require_auth();
        require_not_paused(&env);
        require_admin(&env, &caller);
        let mut record = load_escrow(&env, delivery_id);
        if record.status != EscrowStatus::Paused {
            panic_with_error!(&env, EscrowError::InvalidState);
        }
        if release_to_driver {
            let platform_fee_bps: u32 = env
                .storage()
                .instance()
                .get::<_, ProtocolConfig>(&StorageKey::ProtocolConfig)
                .map(|config| config.platform_fee_bps)
                .unwrap_or(0);
            let platform_fee = calculate_fee(record.amount, platform_fee_bps);
            let driver_amount = record.amount.saturating_sub(platform_fee);

            let fleet_management = get_fleet_management_contract(&env);
            payout_driver(
                &env,
                &record.token,
                &record.driver,
                driver_amount,
                fleet_management.as_ref(),
                record.fleet_id,
            );

            if platform_fee > 0 {
                let admin: Address = env
                    .storage()
                    .instance()
                    .get(&StorageKey::Admin)
                    .expect("Not initialized");
                token::Client::new(&env, &record.token).transfer(
                    &env.current_contract_address(),
                    &admin,
                    &platform_fee,
                );
            }

            record.status = EscrowStatus::Released;
        } else {
            let contract_balance =
                token::Client::new(&env, &record.token).balance(&env.current_contract_address());
            if contract_balance < record.amount {
                panic_with_error!(&env, EscrowError::InsufficientFunds);
            }
            token::Client::new(&env, &record.token).transfer(
                &env.current_contract_address(),
                &record.sender,
                &record.amount,
            );
            record.status = EscrowStatus::Refunded;
        }

        save_escrow(&env, delivery_id, &record);

        env.events().publish(
            (events::dispute_resolved(&env),),
            shared_types::DisputeResolvedEvent {
                delivery_id,
                resolver: caller,
            },
        );
    }

    pub fn resolve_dispute_split(
        env: Env,
        caller: Address,
        delivery_id: u64,
        sender_share_bps: u32,
    ) {
        caller.require_auth();
        require_not_paused(&env);
        require_admin(&env, &caller);
        if sender_share_bps > 10000 {
            panic_with_error!(&env, EscrowError::InvalidFee);
        }
        let mut record = load_escrow(&env, delivery_id);
        if record.status != EscrowStatus::Paused {
            panic_with_error!(&env, EscrowError::InvalidState);
        }
        let contract_balance =
            token::Client::new(&env, &record.token).balance(&env.current_contract_address());
        if contract_balance < record.amount {
            panic_with_error!(&env, EscrowError::InsufficientFunds);
        }

        let sender_amount = record.amount.saturating_mul(sender_share_bps as i128) / 10000;
        let driver_amount = record.amount.saturating_sub(sender_amount);

        if sender_amount > 0 {
            token::Client::new(&env, &record.token).transfer(
                &env.current_contract_address(),
                &record.sender,
                &sender_amount,
            );
        }
        if driver_amount > 0 {
            token::Client::new(&env, &record.token).transfer(
                &env.current_contract_address(),
                &record.driver,
                &driver_amount,
            );
        }

        record.status = EscrowStatus::Split;
        save_escrow(&env, delivery_id, &record);

        env.events().publish(
            (events::dispute_resolved(&env),),
            shared_types::DisputeResolvedEvent {
                delivery_id,
                resolver: caller,
            },
        );
    }

    pub fn release_holdback_escrow(env: Env, caller: Address, delivery_id: u64) {
        caller.require_auth();
        let mut record = load_escrow(&env, delivery_id);
        let admin_authorized = is_admin(&env, &caller);
        let recipient_authorized = caller == record.recipient;
        if !admin_authorized && !recipient_authorized {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }
        if record.status != EscrowStatus::Holdback {
            panic_with_error!(&env, EscrowError::InvalidState);
        }
        // Balance verification guard: confirm contract holds sufficient funds before transfer
        let contract_balance =
            token::Client::new(&env, &record.token).balance(&env.current_contract_address());
        if contract_balance < record.amount {
            panic_with_error!(&env, EscrowError::InsufficientFunds);
        }
        let platform_fee_bps: u32 = env
            .storage()
            .instance()
            .get::<_, ProtocolConfig>(&StorageKey::ProtocolConfig)
            .map(|config| config.platform_fee_bps)
            .unwrap_or(0);
        let platform_fee = calculate_fee(record.amount, platform_fee_bps);
        let driver_amount = record.amount.saturating_sub(platform_fee);

        payout_driver(&env, &record.token, &record.driver, driver_amount);

        if platform_fee > 0 {
            let admin: Address = env
                .storage()
                .instance()
                .get(&StorageKey::Admin)
                .expect("Not initialized");
            token::Client::new(&env, &record.token).transfer(
                &env.current_contract_address(),
                &admin,
                &platform_fee,
            );
        }

        record.status = EscrowStatus::Released;
        save_escrow(&env, delivery_id, &record);
        env.events().publish(
            (events::escrow_released(&env), delivery_id),
            (record.driver, driver_amount, platform_fee),
        );
    }

    pub fn get_escrow(env: Env, delivery_id: u64) -> EscrowRecord {
        if !env.storage().persistent().has(&escrow_key(delivery_id)) {
            panic_with_error!(&env, EscrowError::DeliveryNotFound);
        }
        load_escrow(&env, delivery_id)
    }

    pub fn freeze_funds(env: Env, caller: Address, delivery_id: u64) {
        caller.require_auth();
        let dispute_contract = env
            .storage()
            .instance()
            .get(&DataKey::DisputeResolutionContract)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::NotInitialized));
        if caller != dispute_contract {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }
        let mut record = load_escrow(&env, delivery_id);
        if record.status == EscrowStatus::Locked || record.status == EscrowStatus::Holdback {
            record.status = EscrowStatus::Paused;
            record.disputed_at = Some(env.ledger().timestamp());
            save_escrow(&env, delivery_id, &record);
            env.events().publish(
                (Symbol::new(&env, "funds_frozen"), delivery_id),
                (caller, env.ledger().timestamp()),
            );
        }
    }

    pub fn reclaim_expired_escrow(env: Env, delivery_id: u64) {
        let mut record = load_escrow(&env, delivery_id);
        if record.status != EscrowStatus::Locked {
            panic_with_error!(&env, EscrowError::InvalidState);
        }
        if let Some(expires_at) = record.expires_at {
            let current_timestamp = env.ledger().timestamp();
            if current_timestamp <= expires_at {
                panic_with_error!(&env, EscrowError::InvalidState);
            }
        } else {
            panic_with_error!(&env, EscrowError::InvalidState);
        }
        let contract_balance =
            token::Client::new(&env, &record.token).balance(&env.current_contract_address());
        if contract_balance < record.amount {
            panic_with_error!(&env, EscrowError::InsufficientFunds);
        }
        token::Client::new(&env, &record.token).transfer(
            &env.current_contract_address(),
            &record.sender,
            &record.amount,
        );
        record.status = EscrowStatus::Refunded;
        save_escrow(&env, delivery_id, &record);
        env.events().publish(
            (events::escrow_refunded(&env), delivery_id),
            (record.sender, record.amount),
        );
    }

    /// Get all escrow delivery IDs for a sender.
    pub fn get_escrows_by_sender(env: Env, sender: Address) -> soroban_sdk::Vec<u64> {
        let key = DataKey::EscrowsBySender(sender);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| soroban_sdk::Vec::new(&env))
    }

    /// Get all escrow delivery IDs for a recipient.
    pub fn get_escrows_by_recipient(env: Env, recipient: Address) -> soroban_sdk::Vec<u64> {
        let key = DataKey::EscrowsByRecipient(recipient);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| soroban_sdk::Vec::new(&env))
    }

    /// Get all escrow delivery IDs for a driver.
    pub fn get_escrows_by_driver(env: Env, driver: Address) -> soroban_sdk::Vec<u64> {
        let key = DataKey::EscrowsByDriver(driver);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| soroban_sdk::Vec::new(&env))
    }
}

#[cfg(test)]
mod test;

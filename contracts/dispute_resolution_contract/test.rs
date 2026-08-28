extern crate std;

use super::*;
use shared_types::{DeliveryId, DeliveryRecord, DeliveryStatus};
use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Events, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    xdr, Address, Env, String, TryFromVal, TryIntoVal, Val,
};

fn did(value: u64) -> DeliveryId {
    DeliveryId::from(value)
}

/// Decode every published event into the `(contract, topics, data)` shape the
/// SDK <27 `env.events().all()` returned directly. SDK 27's `ContractEvents`
/// only exposes the raw XDR form, so tests that need to match on topics/data
/// decode it here (mirrors the helper in `fleet_management_contract`).
fn decoded_events(env: &Env) -> std::vec::Vec<(Address, soroban_sdk::Vec<Val>, Val)> {
    let mut out = std::vec::Vec::new();
    for raw in env.events().all().events().iter() {
        let contract_id = raw.contract_id.clone().expect("event missing contract id");
        let address: Address = xdr::ScVal::Address(xdr::ScAddress::Contract(contract_id))
            .try_into_val(env)
            .expect("failed to decode contract address");
        let xdr::ContractEventBody::V0(body) = raw.body.clone();
        let mut topics = soroban_sdk::Vec::new(env);
        for topic in body.topics.iter() {
            topics.push_back(Val::try_from_val(env, topic).expect("failed to decode topic"));
        }
        let data = Val::try_from_val(env, &body.data).expect("failed to decode event data");
        out.push((address, topics, data));
    }
    out
}

#[contract]
pub struct MockDeliveryContract;

#[contractimpl]
impl MockDeliveryContract {
    pub fn get_delivery(env: Env, delivery_id: DeliveryId) -> DeliveryRecord {
        env.storage()
            .instance()
            .get::<_, DeliveryRecord>(&u64::from(delivery_id))
            .unwrap_or_else(|| panic!("DeliveryNotFound"))
    }

    pub fn raise_dispute(env: Env, _caller: Address, delivery_id: DeliveryId) {
        let storage_key = u64::from(delivery_id);
        if env.storage().instance().has(&storage_key) {
            let mut record: DeliveryRecord = env.storage().instance().get(&storage_key).unwrap();
            record.status = shared_types::DeliveryStatus::Disputed;
            env.storage().instance().set(&storage_key, &record);
        }
    }
}

#[contract]
pub struct MockEscrowContract;

#[contractimpl]
impl MockEscrowContract {
    pub fn get_escrow(env: Env, delivery_id: u64) -> shared_types::EscrowRecord {
        env.storage()
            .instance()
            .get(&delivery_id)
            .unwrap_or_else(|| panic!("EscrowNotFound"))
    }

    pub fn resolve_dispute(env: Env, _caller: Address, delivery_id: u64, release_to_driver: bool) {
        if env.storage().instance().has(&delivery_id) {
            let mut record: shared_types::EscrowRecord =
                env.storage().instance().get(&delivery_id).unwrap();
            if release_to_driver {
                record.status = shared_types::EscrowStatus::Released;
            } else {
                record.status = shared_types::EscrowStatus::Refunded;
            }
            env.storage().instance().set(&delivery_id, &record);
        }
    }

    pub fn resolve_dispute_split(
        env: Env,
        _caller: Address,
        delivery_id: u64,
        _sender_share_bps: u32,
    ) {
        if env.storage().instance().has(&delivery_id) {
            let mut record: shared_types::EscrowRecord =
                env.storage().instance().get(&delivery_id).unwrap();
            record.status = shared_types::EscrowStatus::Refunded;
            env.storage().instance().set(&delivery_id, &record);
        }
    }

    pub fn freeze_funds(env: Env, _caller: Address, delivery_id: u64) {
        if env.storage().instance().has(&delivery_id) {
            let mut record: shared_types::EscrowRecord =
                env.storage().instance().get(&delivery_id).unwrap();
            record.status = shared_types::EscrowStatus::Paused;
            env.storage().instance().set(&delivery_id, &record);
        }
    }
}

fn setup_test() -> (
    Env,
    Address, // admin
    Address, // sender
    Address, // recipient
    Address, // driver
    Address, // delivery contract ID
    Address, // escrow contract ID
    DisputeResolutionContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);

    let delivery_id = env.register(MockDeliveryContract, ());
    let escrow_id = env.register(MockEscrowContract, ());
    let dispute_id = env.register(DisputeResolutionContract, ());

    let dispute_client = DisputeResolutionContractClient::new(&env, &dispute_id);

    // Time limit: 1 day (86400 seconds) — MIN_DISPUTE_TIME_LIMIT
    dispute_client.init(&admin, &delivery_id, &escrow_id, &86400, &604800);

    (
        env,
        admin,
        sender,
        recipient,
        driver,
        delivery_id,
        escrow_id,
        dispute_client,
    )
}

fn set_mock_delivery(
    env: &Env,
    delivery_contract_id: &Address,
    delivery_id: DeliveryId,
    record: &DeliveryRecord,
) {
    env.as_contract(delivery_contract_id, || {
        env.storage()
            .instance()
            .set(&u64::from(delivery_id), record);
    });
}

fn set_mock_escrow(
    env: &Env,
    escrow_contract_id: &Address,
    delivery_id: u64,
    record: &shared_types::EscrowRecord,
) {
    env.as_contract(escrow_contract_id, || {
        env.storage().instance().set(&delivery_id, record);
    });
}

fn create_mock_delivery_record(
    env: &Env,
    delivery_id: DeliveryId,
    sender: Address,
    recipient: Address,
    status: DeliveryStatus,
    delivered_at: Option<u64>,
) -> DeliveryRecord {
    let cargo = shared_types::CargoDescriptor {
        weight_grams: 500,
        category: shared_types::CargoCategory::Electronics,
        fragile: true,
    };
    let metadata = shared_types::DeliveryMetadata {
        delivery_id: u64::from(delivery_id),
        origin: String::from_str(env, "Origin"),
        destination: String::from_str(env, "Destination"),
        cargo_description: cargo,
        created_at: env.ledger().timestamp(),
        estimated_delivery: env.ledger().timestamp() + 3600,
    };
    DeliveryRecord {
        delivery_id,
        sender,
        recipient,
        driver: None,
        status,
        metadata,
        created_at: env.ledger().timestamp(),
        delivered_at,
        transit_started_at: None,
    }
}

fn create_mock_escrow_record(
    sender: Address,
    recipient: Address,
    driver: Address,
    token: Address,
    status: shared_types::EscrowStatus,
) -> shared_types::EscrowRecord {
    shared_types::EscrowRecord {
        sender,
        recipient,
        driver,
        token,
        amount: 500,
        status,
        created_at: 0,
        expires_at: None,
        disputed_by: None,
        disputed_at: None,
        fleet_id: None,
    }
}

#[test]
fn test_init_and_setup() {
    let (_env, admin, _, _, _, delivery_id, escrow_id, dispute_client) = setup_test();

    assert_eq!(dispute_client.get_delivery_contract(), delivery_id);
    assert_eq!(dispute_client.get_escrow_contract(), escrow_id);
    assert_eq!(dispute_client.get_dispute_time_limit(), 86400);
    assert!(dispute_client.is_admin(&admin));
}

#[test]
fn test_admin_whitelist_management() {
    let (env, admin, _, _, _, _, _, dispute_client) = setup_test();

    let new_admin = Address::generate(&env);
    assert!(!dispute_client.is_admin(&new_admin));

    // Admin adds new_admin
    dispute_client.add_admin(&admin, &new_admin);
    assert!(dispute_client.is_admin(&new_admin));

    // Original admin steps down, leaving new_admin as the sole admin. A
    // self-removal is the sanctioned way to reduce the roster to one admin
    // (Issue #212); an admin removing a *different* admin may never leave
    // itself alone.
    dispute_client.remove_admin(&admin, &admin);
    assert!(!dispute_client.is_admin(&admin));
    assert!(dispute_client.is_admin(&new_admin));
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #5)")] // FaniLabError::InvalidState
fn test_admin_cannot_consolidate_roster_to_self() {
    // Issue #212: an admin must not be able to reduce the roster to only
    // itself by removing the others.
    let (env, admin, _, _, _, _, _, dispute_client) = setup_test();

    let admin2 = Address::generate(&env);
    dispute_client.add_admin(&admin, &admin2);

    // `admin` removing `admin2` would leave `admin` as the sole admin — the
    // self-service consolidation this guard blocks.
    dispute_client.remove_admin(&admin, &admin2);
}

#[test]
fn test_admin_removal_still_works_while_another_admin_remains() {
    // Issue #212 regression: legitimate removals through the intended process
    // still succeed as long as at least one other admin remains.
    let (env, admin, _, _, _, _, _, dispute_client) = setup_test();

    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);
    dispute_client.add_admin(&admin, &admin2);
    dispute_client.add_admin(&admin, &admin3);

    // Roster is [admin, admin2, admin3]; removing admin3 leaves two admins.
    dispute_client.remove_admin(&admin, &admin3);
    assert!(!dispute_client.is_admin(&admin3));
    assert_eq!(dispute_client.list_admins().len(), 2);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #5)")] // FaniLabError::InvalidState
fn test_remove_last_admin_rejected() {
    let (_env, admin, _, _, _, _, _, dispute_client) = setup_test();

    // `admin` is the only admin left after init — removing it must be
    // rejected, since it would permanently brick governance (no one left
    // who could call add_admin to recover).
    dispute_client.remove_admin(&admin, &admin);
}

#[test]
fn test_remove_admin_allowed_when_multiple_admins_remain() {
    let (env, admin, _, _, _, _, _, dispute_client) = setup_test();

    let second_admin = Address::generate(&env);
    dispute_client.add_admin(&admin, &second_admin);

    // With two admins present, removing one must still succeed.
    dispute_client.remove_admin(&admin, &admin);
    assert!(!dispute_client.is_admin(&admin));
    assert!(dispute_client.is_admin(&second_admin));
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #1)")] // FaniLabError::Unauthorized
fn test_unauthorized_add_admin_fails() {
    let (env, _, sender, _, _, _, _, dispute_client) = setup_test();
    let attacker = sender;
    let target = Address::generate(&env);

    dispute_client.add_admin(&attacker, &target);
}

#[test]
fn test_raise_dispute_active_delivery() {
    let (env, _admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();

    // Setup mock delivery status: Active
    let delivery_record = create_mock_delivery_record(
        &env,
        did(1),
        sender.clone(),
        recipient.clone(),
        DeliveryStatus::Active,
        None,
    );
    set_mock_delivery(&env, &delivery_id, did(1), &delivery_record);

    // Setup mock escrow status: Locked
    let token = Address::generate(&env);
    let escrow_record = create_mock_escrow_record(
        sender.clone(),
        recipient.clone(),
        driver.clone(),
        token,
        shared_types::EscrowStatus::Locked,
    );
    set_mock_escrow(&env, &escrow_id, 1, &escrow_record);

    // Raise dispute
    dispute_client.raise_dispute(&sender, &did(1));

    // Verify delivery status changed to Disputed in MockDeliveryContract
    let delivery = MockDeliveryContractClient::new(&env, &delivery_id).get_delivery(&did(1));
    assert_eq!(delivery.status, DeliveryStatus::Disputed);

    // Verify escrow status changed to Paused in MockEscrowContract
    let escrow = MockEscrowContractClient::new(&env, &escrow_id).get_escrow(&1);
    assert_eq!(escrow.status, shared_types::EscrowStatus::Paused);

    // Verify local dispute case in DisputeResolutionContract
    let case = dispute_client.get_dispute(&did(1));
    assert_eq!(case.delivery_id, did(1));
    assert_eq!(case.status, DisputeStatus::Open);
    assert_eq!(case.raised_by, sender);
    assert_eq!(case.evidence_hashes.len(), 0);
}

#[test]
fn test_raise_dispute_delivered_within_time_limit() {
    let (env, _admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();

    // Setup mock delivery status: Delivered with timestamp
    let delivered_at = env.ledger().timestamp();
    let delivery_record = create_mock_delivery_record(
        &env,
        did(2),
        sender.clone(),
        recipient.clone(),
        DeliveryStatus::Delivered,
        Some(delivered_at),
    );
    set_mock_delivery(&env, &delivery_id, did(2), &delivery_record);

    // Setup mock escrow status: Released
    let token = Address::generate(&env);
    let escrow_record = create_mock_escrow_record(
        sender.clone(),
        recipient.clone(),
        driver.clone(),
        token,
        shared_types::EscrowStatus::Released,
    );
    set_mock_escrow(&env, &escrow_id, 2, &escrow_record);

    // Set time forward by 1800 seconds (30 mins)
    env.ledger().set_timestamp(delivered_at + 1800);

    // Raise dispute
    dispute_client.raise_dispute(&recipient, &did(2));

    // Verify local dispute case is created
    let case = dispute_client.get_dispute(&did(2));
    assert_eq!(case.status, DisputeStatus::Open);
    assert_eq!(case.raised_by, recipient);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #5)")] // FaniLabError::InvalidState
fn test_raise_dispute_delivered_exceeds_time_limit() {
    let (env, _admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();

    // Setup mock delivery status: Delivered
    let delivered_at = env.ledger().timestamp();
    let delivery_record = create_mock_delivery_record(
        &env,
        did(3),
        sender.clone(),
        recipient.clone(),
        DeliveryStatus::Delivered,
        Some(delivered_at),
    );
    set_mock_delivery(&env, &delivery_id, did(3), &delivery_record);

    // Setup mock escrow status: Released
    let token = Address::generate(&env);
    let escrow_record = create_mock_escrow_record(
        sender.clone(),
        recipient.clone(),
        driver.clone(),
        token,
        shared_types::EscrowStatus::Released,
    );
    set_mock_escrow(&env, &escrow_id, 3, &escrow_record);

    // Set time forward past the 86400s (MIN_DISPUTE_TIME_LIMIT) configured in setup_test
    env.ledger().set_timestamp(delivered_at + 86401);

    // Attempt to raise dispute (should fail due to time limit exceeded)
    dispute_client.raise_dispute(&recipient, &did(3));
}

#[test]
fn test_resolve_dispute_refund_sender_by_admin() {
    let (env, admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();

    // Setup mock delivery with driver assigned (required for reputation penalty on resolve)
    let mut delivery_record = create_mock_delivery_record(
        &env,
        did(4),
        sender.clone(),
        recipient.clone(),
        DeliveryStatus::Active,
        None,
    );
    delivery_record.driver = Some(driver.clone());
    set_mock_delivery(&env, &delivery_id, did(4), &delivery_record);

    // Setup mock escrow as Paused (representing escrow paused after dispute raised)
    let token = Address::generate(&env);
    let escrow_record = create_mock_escrow_record(
        sender.clone(),
        recipient.clone(),
        driver.clone(),
        token,
        shared_types::EscrowStatus::Paused,
    );
    set_mock_escrow(&env, &escrow_id, 4, &escrow_record);

    // Raise dispute to initialize local dispute case
    dispute_client.raise_dispute(&sender, &did(4));

    // Resolve dispute
    dispute_client.resolve_dispute_refund_sender(&admin, &did(4));

    // Verify local dispute status is ResolvedRefund
    let case = dispute_client.get_dispute(&did(4));
    assert_eq!(case.status, DisputeStatus::ResolvedRefund);

    // Verify mock escrow status updated to Refunded
    let escrow = MockEscrowContractClient::new(&env, &escrow_id).get_escrow(&4);
    assert_eq!(escrow.status, shared_types::EscrowStatus::Refunded);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #1)")] // FaniLabError::Unauthorized
fn test_unauthorized_resolve_dispute_fails() {
    let (env, _admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();

    let delivery_record = create_mock_delivery_record(
        &env,
        did(5),
        sender.clone(),
        recipient.clone(),
        DeliveryStatus::Active,
        None,
    );
    set_mock_delivery(&env, &delivery_id, did(5), &delivery_record);

    let token = Address::generate(&env);
    let escrow_record = create_mock_escrow_record(
        sender.clone(),
        recipient.clone(),
        driver.clone(),
        token,
        shared_types::EscrowStatus::Paused,
    );
    set_mock_escrow(&env, &escrow_id, 5, &escrow_record);

    dispute_client.raise_dispute(&sender, &did(5));

    // Attacker (sender) tries to resolve dispute
    dispute_client.resolve_dispute_refund_sender(&sender, &did(5));
}

#[test]
fn test_add_evidence_hash_success() {
    let (env, _admin, sender, recipient, _driver, delivery_id, _escrow_id, dispute_client) =
        setup_test();

    let delivery_record = create_mock_delivery_record(
        &env,
        did(6),
        sender.clone(),
        recipient.clone(),
        DeliveryStatus::Active,
        None,
    );
    set_mock_delivery(&env, &delivery_id, did(6), &delivery_record);

    dispute_client.raise_dispute(&sender, &did(6));

    let evidence_hash1 = soroban_sdk::BytesN::from_array(&env, &[1; 32]);
    let evidence_hash2 = soroban_sdk::BytesN::from_array(&env, &[2; 32]);

    // Sender adds evidence
    dispute_client.add_evidence_hash(&sender, &did(6), &evidence_hash1);
    // Recipient adds evidence
    dispute_client.add_evidence_hash(&recipient, &did(6), &evidence_hash2);

    let case = dispute_client.get_dispute(&did(6));
    assert_eq!(case.evidence_hashes.len(), 2);
    assert_eq!(case.evidence_hashes.get(0).unwrap(), evidence_hash1);
    assert_eq!(case.evidence_hashes.get(1).unwrap(), evidence_hash2);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #1)")] // FaniLabError::Unauthorized
fn test_add_evidence_unauthorized_fails() {
    let (env, _admin, sender, recipient, _driver, delivery_id, _escrow_id, dispute_client) =
        setup_test();

    let delivery_record = create_mock_delivery_record(
        &env,
        did(7),
        sender.clone(),
        recipient.clone(),
        DeliveryStatus::Active,
        None,
    );
    set_mock_delivery(&env, &delivery_id, did(7), &delivery_record);

    dispute_client.raise_dispute(&sender, &did(7));

    let attacker = Address::generate(&env);
    let evidence_hash = soroban_sdk::BytesN::from_array(&env, &[3; 32]);

    dispute_client.add_evidence_hash(&attacker, &did(7), &evidence_hash);
}

#[test]
fn test_add_evidence_hash_up_to_cap_succeeds() {
    let (env, _admin, sender, recipient, _driver, delivery_id, _escrow_id, dispute_client) =
        setup_test();

    let delivery_record = create_mock_delivery_record(
        &env,
        did(8),
        sender.clone(),
        recipient.clone(),
        DeliveryStatus::Active,
        None,
    );
    set_mock_delivery(&env, &delivery_id, did(8), &delivery_record);

    dispute_client.raise_dispute(&sender, &did(8));

    for i in 0..MAX_EVIDENCE_HASHES {
        let hash = soroban_sdk::BytesN::from_array(&env, &[i as u8; 32]);
        dispute_client.add_evidence_hash(&sender, &did(8), &hash);
    }

    let case = dispute_client.get_dispute(&did(8));
    assert_eq!(case.evidence_hashes.len(), MAX_EVIDENCE_HASHES);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #12)")] // FaniLabError::LimitExceeded
fn test_add_evidence_hash_beyond_cap_rejected() {
    let (env, _admin, sender, recipient, _driver, delivery_id, _escrow_id, dispute_client) =
        setup_test();

    let delivery_record = create_mock_delivery_record(
        &env,
        did(9),
        sender.clone(),
        recipient.clone(),
        DeliveryStatus::Active,
        None,
    );
    set_mock_delivery(&env, &delivery_id, did(9), &delivery_record);

    dispute_client.raise_dispute(&sender, &did(9));

    for i in 0..MAX_EVIDENCE_HASHES {
        let hash = soroban_sdk::BytesN::from_array(&env, &[i as u8; 32]);
        dispute_client.add_evidence_hash(&sender, &did(9), &hash);
    }

    // One past the cap must be rejected.
    let one_too_many = soroban_sdk::BytesN::from_array(&env, &[0xFF; 32]);
    dispute_client.add_evidence_hash(&sender, &did(9), &one_too_many);
}

#[test]
fn test_integration_resolve_dispute_split_funds() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);

    // Register real contracts
    let delivery_contract_id = env.register(delivery_contract::DeliveryContract, ());
    let escrow_contract_id = env.register(escrow_contract::EscrowContract, ());
    let dispute_resolution_id = env.register(DisputeResolutionContract, ());

    let delivery_client =
        delivery_contract::DeliveryContractClient::new(&env, &delivery_contract_id);
    let escrow_client = escrow_contract::EscrowContractClient::new(&env, &escrow_contract_id);
    let dispute_client = DisputeResolutionContractClient::new(&env, &dispute_resolution_id);

    // Register stellar asset contract for token
    let token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    // Init contracts
    escrow_client.init(&admin, &token, &0);
    escrow_client.set_dispute_resolution_contract(&admin, &dispute_resolution_id);
    delivery_client.init(&admin, &escrow_contract_id);
    dispute_client.init(
        &admin,
        &delivery_contract_id,
        &escrow_contract_id,
        &86400,
        &604800,
    );

    // Mint tokens to sender
    StellarAssetClient::new(&env, &token).mint(&sender, &1000);

    // Create delivery
    let metadata = {
        let cargo = shared_types::CargoDescriptor {
            weight_grams: 500,
            category: shared_types::CargoCategory::Electronics,
            fragile: true,
        };
        shared_types::DeliveryMetadata {
            delivery_id: 0,
            origin: String::from_str(&env, "Origin"),
            destination: String::from_str(&env, "Destination"),
            cargo_description: cargo,
            created_at: env.ledger().timestamp(),
            estimated_delivery: env.ledger().timestamp() + 3600,
        }
    };
    let delivery_id_val = delivery_client.create_delivery(&sender, &recipient, &metadata);

    // Create escrow
    escrow_client.create_escrow(
        &sender,
        &recipient,
        &driver,
        &u64::from(delivery_id_val),
        &token,
        &1000,
        &None,
    );

    // Assign driver to make delivery Active
    delivery_client.assign_driver(&admin, &delivery_id_val, &driver);

    // Raise dispute
    dispute_client.raise_dispute(&sender, &delivery_id_val);

    // Verify escrow is paused
    let escrow = escrow_client.get_escrow(&u64::from(delivery_id_val));
    assert_eq!(escrow.status, shared_types::EscrowStatus::Paused);

    // Resolve split (60% sender, 40% driver)
    dispute_client.resolve_dispute_split_funds(&admin, &delivery_id_val, &6000);

    // Verify local dispute is Split
    let case = dispute_client.get_dispute(&delivery_id_val);
    assert_eq!(case.status, DisputeStatus::Split);

    // Verify token balances
    let sender_balance = TokenClient::new(&env, &token).balance(&sender);
    let driver_balance = TokenClient::new(&env, &token).balance(&driver);
    assert_eq!(sender_balance, 600); // 60% of 1000 refunded
    assert_eq!(driver_balance, 400); // 40% of 1000 paid to driver
}

/// Issue #51 regression test: `resolve_dispute_refund_sender` is the one path
/// in the protocol that cross-calls `identity_reputation_contract::
/// decrease_reputation`, but until now nothing exercised it end-to-end
/// through real contracts — a full delivery -> escrow -> dispute_resolution
/// -> identity_reputation chain. This wires all four real contracts together
/// and asserts the driver's on-chain reputation score actually drops.
#[test]
fn test_integration_resolve_dispute_refund_sender_decreases_reputation() {
    use identity_reputation_contract::{
        IdentityReputationContract, IdentityReputationContractClient,
    };

    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);

    // Register real contracts.
    let delivery_contract_id = env.register(delivery_contract::DeliveryContract, ());
    let escrow_contract_id = env.register(escrow_contract::EscrowContract, ());
    let dispute_resolution_id = env.register(DisputeResolutionContract, ());
    let identity_contract_id = env.register(IdentityReputationContract, ());

    let delivery_client =
        delivery_contract::DeliveryContractClient::new(&env, &delivery_contract_id);
    let escrow_client = escrow_contract::EscrowContractClient::new(&env, &escrow_contract_id);
    let dispute_client = DisputeResolutionContractClient::new(&env, &dispute_resolution_id);
    let identity_client = IdentityReputationContractClient::new(&env, &identity_contract_id);

    let token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    escrow_client.init(&admin, &token, &0);
    escrow_client.set_dispute_resolution_contract(&admin, &dispute_resolution_id);
    delivery_client.init(&admin, &escrow_contract_id);
    dispute_client.init(
        &admin,
        &delivery_contract_id,
        &escrow_contract_id,
        &86400,
        &604800,
    );
    // Authorizes both delivery_contract and dispute_resolution_contract to
    // call increase_reputation/decrease_reputation.
    identity_client.init(&admin, &delivery_contract_id, &dispute_resolution_id);
    dispute_client.set_identity_reputation_contract(&admin, &identity_contract_id);

    identity_client.register_driver(&driver);
    assert_eq!(
        identity_client.get_driver_profile(&driver).reputation_score,
        50
    );

    StellarAssetClient::new(&env, &token).mint(&sender, &1000);

    let metadata = {
        let cargo = shared_types::CargoDescriptor {
            weight_grams: 500,
            category: shared_types::CargoCategory::Electronics,
            fragile: false,
        };
        shared_types::DeliveryMetadata {
            delivery_id: 0,
            origin: String::from_str(&env, "Origin"),
            destination: String::from_str(&env, "Destination"),
            cargo_description: cargo,
            created_at: env.ledger().timestamp(),
            estimated_delivery: env.ledger().timestamp() + 3600,
        }
    };
    let delivery_id_val = delivery_client.create_delivery(&sender, &recipient, &metadata);

    escrow_client.create_escrow(
        &sender,
        &recipient,
        &driver,
        &u64::from(delivery_id_val),
        &token,
        &1000,
        &None,
    );

    delivery_client.assign_driver(&admin, &delivery_id_val, &driver);
    dispute_client.raise_dispute(&sender, &delivery_id_val);

    dispute_client.resolve_dispute_refund_sender(&admin, &delivery_id_val);

    let case = dispute_client.get_dispute(&delivery_id_val);
    assert_eq!(case.status, DisputeStatus::ResolvedRefund);

    let penalty = dispute_client.get_dispute_reputation_penalty();
    assert_eq!(
        identity_client.get_driver_profile(&driver).reputation_score,
        50 - penalty
    );

    // The sender got their funds back; the driver's reputation is the only
    // thing that moved.
    assert_eq!(TokenClient::new(&env, &token).balance(&sender), 1000);
}

#[test]
fn test_resolve_dispute_pay_driver_by_admin() {
    let (env, admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();

    // Setup mock delivery
    let delivery_record = create_mock_delivery_record(
        &env,
        did(8),
        sender.clone(),
        recipient.clone(),
        DeliveryStatus::Active,
        None,
    );
    set_mock_delivery(&env, &delivery_id, did(8), &delivery_record);

    // Setup mock escrow as Paused (representing escrow paused after dispute raised)
    let token = Address::generate(&env);
    let escrow_record = create_mock_escrow_record(
        sender.clone(),
        recipient.clone(),
        driver.clone(),
        token,
        shared_types::EscrowStatus::Paused,
    );
    set_mock_escrow(&env, &escrow_id, 8, &escrow_record);

    // Raise dispute to initialize local dispute case
    dispute_client.raise_dispute(&sender, &did(8));

    // Resolve dispute
    dispute_client.resolve_dispute_pay_driver(&admin, &did(8));

    // Verify local dispute status is ResolvedPayout
    let case = dispute_client.get_dispute(&did(8));
    assert_eq!(case.status, DisputeStatus::ResolvedPayout);

    // Verify mock escrow status updated to Released
    let escrow = MockEscrowContractClient::new(&env, &escrow_id).get_escrow(&8);
    assert_eq!(escrow.status, shared_types::EscrowStatus::Released);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #1)")] // FaniLabError::Unauthorized
fn test_unauthorized_resolve_pay_driver_fails() {
    let (env, _admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();

    let delivery_record = create_mock_delivery_record(
        &env,
        did(9),
        sender.clone(),
        recipient.clone(),
        DeliveryStatus::Active,
        None,
    );
    set_mock_delivery(&env, &delivery_id, did(9), &delivery_record);

    let token = Address::generate(&env);
    let escrow_record = create_mock_escrow_record(
        sender.clone(),
        recipient.clone(),
        driver.clone(),
        token,
        shared_types::EscrowStatus::Paused,
    );
    set_mock_escrow(&env, &escrow_id, 9, &escrow_record);

    dispute_client.raise_dispute(&sender, &did(9));

    // Attacker (sender) tries to resolve dispute pay driver
    dispute_client.resolve_dispute_pay_driver(&sender, &did(9));
}

#[test]
fn test_dispute_reputation_penalty_configurable() {
    let (_env, admin, _, _, _, _, _, dispute_client) = setup_test();

    // Default matches the previously hardcoded value
    assert_eq!(dispute_client.get_dispute_reputation_penalty(), 10);

    dispute_client.set_dispute_reputation_penalty(&admin, &25);
    assert_eq!(dispute_client.get_dispute_reputation_penalty(), 25);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #1)")] // FaniLabError::Unauthorized
fn test_unauthorized_set_dispute_reputation_penalty_fails() {
    let (_env, _admin, sender, _, _, _, _, dispute_client) = setup_test();

    dispute_client.set_dispute_reputation_penalty(&sender, &25);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #1)")] // FaniLabError::Unauthorized
fn test_unauthorized_resolve_split_funds_fails() {
    let (_env, _admin, sender, _recipient, _driver, _delivery_id, _escrow_id, dispute_client) =
        setup_test();

    dispute_client.resolve_dispute_split_funds(&sender, &did(10), &5000);
}

// ── DISPUTE TIME LIMIT VALIDATION (Issue #21) ────────────────────────────────

#[test]
#[should_panic(expected = "HostError: Error(Contract, #5)")] // InvalidState
fn test_init_with_zero_dispute_time_limit() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let delivery_id = env.register(MockDeliveryContract, ());
    let escrow_id = env.register(MockEscrowContract, ());
    let dispute_id = env.register(DisputeResolutionContract, ());

    let dispute_client = DisputeResolutionContractClient::new(&env, &dispute_id);

    // Attempt to init with dispute_time_limit = 0 (should fail)
    dispute_client.init(&admin, &delivery_id, &escrow_id, &0, &604800);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #5)")] // InvalidState
fn test_init_with_below_minimum_dispute_time_limit() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let delivery_id = env.register(MockDeliveryContract, ());
    let escrow_id = env.register(MockEscrowContract, ());
    let dispute_id = env.register(DisputeResolutionContract, ());

    let dispute_client = DisputeResolutionContractClient::new(&env, &dispute_id);

    // Attempt to init with dispute_time_limit below minimum (should fail)
    dispute_client.init(&admin, &delivery_id, &escrow_id, &1000, &604800);
}

#[test]
fn test_init_with_minimum_dispute_time_limit() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let delivery_id = env.register(MockDeliveryContract, ());
    let escrow_id = env.register(MockEscrowContract, ());
    let dispute_id = env.register(DisputeResolutionContract, ());

    let dispute_client = DisputeResolutionContractClient::new(&env, &dispute_id);

    // Init with minimum dispute_time_limit should succeed
    dispute_client.init(&admin, &delivery_id, &escrow_id, &86400, &604800);

    let limit = dispute_client.get_dispute_time_limit();
    assert_eq!(limit, 86400);
}

// ── SPLIT RESOLUTION PRECONDITION (Issue #22) ────────────────────────────────

#[test]
#[should_panic(expected = "HostError: Error(Contract, #5)")] // InvalidState
fn test_split_resolve_with_non_paused_escrow_fails() {
    let (env, admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();

    let token = Address::generate(&env);
    let escrow_record = create_mock_escrow_record(
        sender.clone(),
        recipient.clone(),
        driver.clone(),
        token,
        shared_types::EscrowStatus::Locked, // NOT Paused
    );
    set_mock_escrow(&env, &escrow_id, 10, &escrow_record);

    let delivery_record = create_mock_delivery_record(
        &env,
        did(10),
        sender.clone(),
        recipient.clone(),
        DeliveryStatus::Active,
        None,
    );
    set_mock_delivery(&env, &delivery_id, did(10), &delivery_record);

    // Raise dispute to create the dispute case (this also pauses the mock
    // escrow via freeze_funds, so reset it back to Locked afterward to
    // exercise the non-Paused guard in resolve_dispute_split_funds).
    dispute_client.raise_dispute(&sender, &did(10));
    set_mock_escrow(&env, &escrow_id, 10, &escrow_record);

    // Attempt to split-resolve with non-Paused escrow should fail loudly
    dispute_client.resolve_dispute_split_funds(&admin, &did(10), &5000);
}

#[test]
fn test_post_delivery_dispute_can_be_raised_and_resolved() {
    let (env, admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();

    // Setup mock delivery as Delivered (post-delivery state)
    let delivered_at = env.ledger().timestamp();
    let mut delivery_record = create_mock_delivery_record(
        &env,
        did(10),
        sender.clone(),
        recipient.clone(),
        DeliveryStatus::Delivered,
        Some(delivered_at),
    );
    delivery_record.driver = Some(driver.clone());
    set_mock_delivery(&env, &delivery_id, did(10), &delivery_record);

    // Setup mock escrow as Holdback (post-delivery, before release)
    let token = Address::generate(&env);
    let escrow_record = create_mock_escrow_record(
        sender.clone(),
        recipient.clone(),
        driver.clone(),
        token.clone(),
        shared_types::EscrowStatus::Holdback,
    );
    set_mock_escrow(&env, &escrow_id, 10, &escrow_record);

    // Raise dispute on delivered delivery within time limit
    dispute_client.raise_dispute(&sender, &did(10));

    // Verify dispute is created
    let case = dispute_client.get_dispute(&did(10));
    assert_eq!(case.status, DisputeStatus::Open);

    // Verify escrow is paused (frozen for dispute)
    let escrow = MockEscrowContractClient::new(&env, &escrow_id).get_escrow(&10);
    assert_eq!(escrow.status, shared_types::EscrowStatus::Paused);

    // Resolve dispute refunding sender
    dispute_client.resolve_dispute_refund_sender(&admin, &did(10));

    // Verify dispute is resolved
    let case = dispute_client.get_dispute(&did(10));
    assert_eq!(case.status, DisputeStatus::ResolvedRefund);

    // Verify escrow is refunded
    let escrow = MockEscrowContractClient::new(&env, &escrow_id).get_escrow(&10);
    assert_eq!(escrow.status, shared_types::EscrowStatus::Refunded);
}

// ── ADMIN LIST ENUMERATION TESTS ──────────────────────────────────────────────

#[test]
fn test_list_admins_returns_initial_admin() {
    let (_env, admin, _sender, _recipient, _driver, _delivery_id, _escrow_id, dispute_client) =
        setup_test();

    let admins = dispute_client.list_admins();
    assert_eq!(admins.len(), 1);
    assert_eq!(admins.get(0).unwrap(), admin);
}

#[test]
fn test_list_admins_after_adding_admin() {
    let (env, admin, _sender, _recipient, _driver, _delivery_id, _escrow_id, dispute_client) =
        setup_test();

    let new_admin = Address::generate(&env);
    dispute_client.add_admin(&admin, &new_admin);

    let admins = dispute_client.list_admins();
    assert_eq!(admins.len(), 2);
    assert_eq!(admins.get(0).unwrap(), admin);
    assert_eq!(admins.get(1).unwrap(), new_admin);
}

#[test]
fn test_list_admins_after_removing_admin() {
    let (env, admin, _sender, _recipient, _driver, _delivery_id, _escrow_id, dispute_client) =
        setup_test();

    let new_admin = Address::generate(&env);
    dispute_client.add_admin(&admin, &new_admin);
    // `new_admin` steps down; reducing the roster to a single admin is only
    // permitted via self-removal (Issue #212).
    dispute_client.remove_admin(&new_admin, &new_admin);

    let admins = dispute_client.list_admins();
    assert_eq!(admins.len(), 1);
    assert_eq!(admins.get(0).unwrap(), admin);
}

#[test]
fn test_list_admins_after_multiple_additions_and_removals() {
    let (env, admin, _sender, _recipient, _driver, _delivery_id, _escrow_id, dispute_client) =
        setup_test();

    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);
    let admin4 = Address::generate(&env);

    dispute_client.add_admin(&admin, &admin2);
    dispute_client.add_admin(&admin2, &admin3);
    dispute_client.add_admin(&admin3, &admin4);

    let admins = dispute_client.list_admins();
    assert_eq!(admins.len(), 4);

    dispute_client.remove_admin(&admin2, &admin3);
    let admins = dispute_client.list_admins();
    assert_eq!(admins.len(), 3);
}

// ── Issue #211: uniform "escrow must be Paused" precondition ────────────────
//
// Only `resolve_dispute_split_funds` previously fetched the escrow and
// asserted `Paused`. `resolve_dispute_refund_sender` and
// `resolve_dispute_pay_driver` relied on `escrow_contract`'s own guard,
// which fires only after a cross-contract reputation adjustment has been
// attempted. All three now share `require_escrow_paused`, called before any
// state mutation or side effect.

#[contract]
pub struct MockReputationContract;

#[contractimpl]
impl MockReputationContract {
    fn bump(env: &Env) {
        let k = Symbol::new(env, "calls");
        let n: u32 = env.storage().instance().get(&k).unwrap_or(0);
        env.storage().instance().set(&k, &(n + 1));
    }

    pub fn decrease_reputation(env: Env, _caller: Address, _subject: Address, _amount: u32) {
        Self::bump(&env);
    }

    pub fn increase_reputation(env: Env, _caller: Address, _subject: Address, _amount: u32) {
        Self::bump(&env);
    }

    pub fn calls(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&Symbol::new(&env, "calls"))
            .unwrap_or(0)
    }
}

/// Register the dispute case for `delivery_id` and then force the mock escrow
/// back to a non-Paused state, so a resolution entry point can be exercised
/// against the bad precondition.
#[allow(clippy::too_many_arguments)]
fn open_dispute_with_non_paused_escrow(
    env: &Env,
    delivery_id: DeliveryId,
    sender: &Address,
    recipient: &Address,
    driver: &Address,
    delivery_contract_id: &Address,
    escrow_contract_id: &Address,
    dispute_client: &DisputeResolutionContractClient,
) {
    let mut delivery_record = create_mock_delivery_record(
        env,
        delivery_id,
        sender.clone(),
        recipient.clone(),
        DeliveryStatus::Active,
        None,
    );
    delivery_record.driver = Some(driver.clone());
    set_mock_delivery(env, delivery_contract_id, delivery_id, &delivery_record);

    let token = Address::generate(env);
    let locked = create_mock_escrow_record(
        sender.clone(),
        recipient.clone(),
        driver.clone(),
        token,
        shared_types::EscrowStatus::Locked,
    );
    set_mock_escrow(env, escrow_contract_id, u64::from(delivery_id), &locked);

    // raise_dispute pauses the mock escrow via freeze_funds; reset it to
    // Locked afterwards to exercise the non-Paused guard.
    dispute_client.raise_dispute(sender, &delivery_id);
    set_mock_escrow(env, escrow_contract_id, u64::from(delivery_id), &locked);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #5)")] // InvalidState
fn test_resolve_refund_sender_rejects_non_paused_escrow() {
    let (env, admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();

    open_dispute_with_non_paused_escrow(
        &env,
        did(30),
        &sender,
        &recipient,
        &driver,
        &delivery_id,
        &escrow_id,
        &dispute_client,
    );

    dispute_client.resolve_dispute_refund_sender(&admin, &did(30));
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #5)")] // InvalidState
fn test_resolve_pay_driver_rejects_non_paused_escrow() {
    let (env, admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();

    open_dispute_with_non_paused_escrow(
        &env,
        did(31),
        &sender,
        &recipient,
        &driver,
        &delivery_id,
        &escrow_id,
        &dispute_client,
    );

    dispute_client.resolve_dispute_pay_driver(&admin, &did(31));
}

#[test]
fn test_resolve_refund_sender_non_paused_makes_no_reputation_change() {
    let (env, admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();

    let reputation_id = env.register(MockReputationContract, ());
    dispute_client.set_identity_reputation_contract(&admin, &reputation_id);

    open_dispute_with_non_paused_escrow(
        &env,
        did(32),
        &sender,
        &recipient,
        &driver,
        &delivery_id,
        &escrow_id,
        &dispute_client,
    );

    let result = dispute_client.try_resolve_dispute_refund_sender(&admin, &did(32));
    assert!(result.is_err());

    // The precondition fails fast: the dispute is untouched and the
    // reputation contract was never called.
    assert_eq!(
        dispute_client.get_dispute(&did(32)).status,
        DisputeStatus::Open
    );
    assert_eq!(
        MockReputationContractClient::new(&env, &reputation_id).calls(),
        0
    );
}

#[test]
fn test_resolve_pay_driver_non_paused_makes_no_reputation_change() {
    let (env, admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();

    let reputation_id = env.register(MockReputationContract, ());
    dispute_client.set_identity_reputation_contract(&admin, &reputation_id);

    open_dispute_with_non_paused_escrow(
        &env,
        did(33),
        &sender,
        &recipient,
        &driver,
        &delivery_id,
        &escrow_id,
        &dispute_client,
    );

    let result = dispute_client.try_resolve_dispute_pay_driver(&admin, &did(33));
    assert!(result.is_err());

    assert_eq!(
        dispute_client.get_dispute(&did(33)).status,
        DisputeStatus::Open
    );
    assert_eq!(
        MockReputationContractClient::new(&env, &reputation_id).calls(),
        0
    );
}

#[test]
fn test_all_resolution_entry_points_succeed_against_paused_escrow() {
    // Regression: the shared precondition does not break the happy path for
    // any of the three entry points.
    for (idx, which) in ["refund", "pay", "split"].iter().enumerate() {
        let (env, admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
            setup_test();
        let d = did(40 + idx as u64);

        let mut delivery_record = create_mock_delivery_record(
            &env,
            d,
            sender.clone(),
            recipient.clone(),
            DeliveryStatus::Active,
            None,
        );
        delivery_record.driver = Some(driver.clone());
        set_mock_delivery(&env, &delivery_id, d, &delivery_record);

        let token = Address::generate(&env);
        let escrow_record = create_mock_escrow_record(
            sender.clone(),
            recipient.clone(),
            driver.clone(),
            token,
            shared_types::EscrowStatus::Paused,
        );
        set_mock_escrow(&env, &escrow_id, u64::from(d), &escrow_record);

        dispute_client.raise_dispute(&sender, &d);

        match *which {
            "refund" => {
                dispute_client.resolve_dispute_refund_sender(&admin, &d);
                assert_eq!(
                    dispute_client.get_dispute(&d).status,
                    DisputeStatus::ResolvedRefund
                );
            }
            "pay" => {
                dispute_client.resolve_dispute_pay_driver(&admin, &d);
                assert_eq!(
                    dispute_client.get_dispute(&d).status,
                    DisputeStatus::ResolvedPayout
                );
            }
            _ => {
                dispute_client.resolve_dispute_split_funds(&admin, &d, &5000);
                assert_eq!(dispute_client.get_dispute(&d).status, DisputeStatus::Split);
            }
        }
    }
}

// ── Issue #213: force_resolve_dispute coverage + overflow hardening ─────────

/// Register an Open dispute for `delivery_id` backed by an Active delivery
/// (with `driver` assigned) and a mock escrow that `raise_dispute` leaves in
/// `Paused`. Returns the `raised_at` timestamp of the new dispute.
#[allow(clippy::too_many_arguments)]
fn open_dispute_for_force_resolve(
    env: &Env,
    delivery_id: DeliveryId,
    sender: &Address,
    recipient: &Address,
    driver: &Address,
    delivery_contract_id: &Address,
    escrow_contract_id: &Address,
    dispute_client: &DisputeResolutionContractClient,
) -> u64 {
    let mut delivery_record = create_mock_delivery_record(
        env,
        delivery_id,
        sender.clone(),
        recipient.clone(),
        DeliveryStatus::Active,
        None,
    );
    delivery_record.driver = Some(driver.clone());
    set_mock_delivery(env, delivery_contract_id, delivery_id, &delivery_record);

    let token = Address::generate(env);
    let escrow_record = create_mock_escrow_record(
        sender.clone(),
        recipient.clone(),
        driver.clone(),
        token,
        shared_types::EscrowStatus::Locked,
    );
    set_mock_escrow(
        env,
        escrow_contract_id,
        u64::from(delivery_id),
        &escrow_record,
    );

    dispute_client.raise_dispute(sender, &delivery_id);
    dispute_client.get_dispute(&delivery_id).raised_at
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #5)")] // InvalidState
fn test_force_resolve_before_window_rejected() {
    let (env, _admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();
    open_dispute_for_force_resolve(
        &env,
        did(50),
        &sender,
        &recipient,
        &driver,
        &delivery_id,
        &escrow_id,
        &dispute_client,
    );

    // No time has elapsed — well within the 604800s resolution window.
    dispute_client.force_resolve_dispute(&sender, &did(50));
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #5)")] // InvalidState
fn test_force_resolve_at_window_boundary_rejected() {
    let (env, _admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();
    let raised_at = open_dispute_for_force_resolve(
        &env,
        did(51),
        &sender,
        &recipient,
        &driver,
        &delivery_id,
        &escrow_id,
        &dispute_client,
    );

    // Exactly at raised_at + resolution_limit: the check is `<=`, so the
    // boundary is still rejected.
    env.ledger().set_timestamp(raised_at + 604800);
    dispute_client.force_resolve_dispute(&sender, &did(51));
}

#[test]
fn test_force_resolve_after_window_proceeds() {
    let (env, _admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();
    let raised_at = open_dispute_for_force_resolve(
        &env,
        did(52),
        &sender,
        &recipient,
        &driver,
        &delivery_id,
        &escrow_id,
        &dispute_client,
    );

    let resolved_time = raised_at + 604800 + 1;
    env.ledger().set_timestamp(resolved_time);
    dispute_client.force_resolve_dispute(&sender, &did(52));

    let case = dispute_client.get_dispute(&did(52));
    assert_eq!(case.status, DisputeStatus::Split);
    assert_eq!(case.resolved_at, Some(resolved_time));
    assert_eq!(case.resolved_by, Some(sender.clone()));

    // The Paused escrow was split-resolved (mock marks it Refunded).
    let escrow = MockEscrowContractClient::new(&env, &escrow_id).get_escrow(&52);
    assert_eq!(escrow.status, shared_types::EscrowStatus::Refunded);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #1)")] // Unauthorized
fn test_force_resolve_by_non_party_rejected() {
    let (env, _admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();
    let raised_at = open_dispute_for_force_resolve(
        &env,
        did(53),
        &sender,
        &recipient,
        &driver,
        &delivery_id,
        &escrow_id,
        &dispute_client,
    );

    env.ledger().set_timestamp(raised_at + 604800 + 1);
    let stranger = Address::generate(&env);
    dispute_client.force_resolve_dispute(&stranger, &did(53));
}

#[test]
fn test_force_resolve_accepted_from_each_party() {
    for (idx, role) in ["sender", "recipient", "driver"].iter().enumerate() {
        let (env, _admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
            setup_test();
        let d = did(54 + idx as u64);
        let raised_at = open_dispute_for_force_resolve(
            &env,
            d,
            &sender,
            &recipient,
            &driver,
            &delivery_id,
            &escrow_id,
            &dispute_client,
        );
        env.ledger().set_timestamp(raised_at + 604800 + 1);

        let caller = match *role {
            "sender" => sender.clone(),
            "recipient" => recipient.clone(),
            _ => driver.clone(),
        };
        dispute_client.force_resolve_dispute(&caller, &d);
        assert_eq!(dispute_client.get_dispute(&d).status, DisputeStatus::Split);
    }
}

#[test]
fn test_force_resolve_requires_open_dispute() {
    let (env, _admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();
    let raised_at = open_dispute_for_force_resolve(
        &env,
        did(57),
        &sender,
        &recipient,
        &driver,
        &delivery_id,
        &escrow_id,
        &dispute_client,
    );
    env.ledger().set_timestamp(raised_at + 604800 + 1);

    // First force-resolution succeeds and moves the dispute out of Open.
    dispute_client.force_resolve_dispute(&sender, &did(57));

    // A second call must be rejected on the state precondition.
    let result = dispute_client.try_force_resolve_dispute(&recipient, &did(57));
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::InvalidState.into()),
        _ => panic!("Expected FaniLabError::InvalidState"),
    }
}

#[test]
fn test_force_resolve_large_resolution_limit_does_not_panic() {
    let (env, admin, sender, recipient, driver, delivery_id, escrow_id, dispute_client) =
        setup_test();

    // Raise the dispute at a non-zero timestamp so `raised_at + limit` would
    // overflow u64 without saturating arithmetic.
    env.ledger().set_timestamp(1_000);
    let raised_at = open_dispute_for_force_resolve(
        &env,
        did(58),
        &sender,
        &recipient,
        &driver,
        &delivery_id,
        &escrow_id,
        &dispute_client,
    );
    assert_eq!(raised_at, 1_000);

    // `set_dispute_resolution_limit` accepts any u64 (see issue #208).
    dispute_client.set_dispute_resolution_limit(&admin, &u64::MAX);
    env.ledger().set_timestamp(2_000);

    // With `saturating_add`, `raised_at + u64::MAX` clamps to u64::MAX and the
    // deadline check simply rejects the call instead of panicking on overflow.
    let result = dispute_client.try_force_resolve_dispute(&sender, &did(58));
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::InvalidState.into()),
        _ => panic!("Expected FaniLabError::InvalidState (not an arithmetic overflow)"),
    }
}

// ── Issue #212: admin roster events ───────────────────────────────────────

/// Assert that the most recent event published by `contract` has `topic`
/// as its first topic and `(caller, affected)` as its data payload. The SDK
/// 27 test env only retains events from the last top-level invocation, so
/// this is checked immediately after each roster call.
fn assert_roster_event(
    env: &Env,
    contract: &Address,
    topic: &str,
    caller: &Address,
    affected: &Address,
) {
    let events = decoded_events(env);
    let (cid, topics, data) = events.last().cloned().expect("no event was published");
    assert_eq!(&cid, contract, "event came from an unexpected contract");
    let topic0: Symbol = Symbol::try_from_val(env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic0, Symbol::new(env, topic), "unexpected event topic");
    let (evt_caller, evt_affected): (Address, Address) =
        <(Address, Address)>::try_from_val(env, &data).unwrap();
    assert_eq!(&evt_caller, caller, "event does not identify the caller");
    assert_eq!(
        &evt_affected, affected,
        "event does not identify the affected admin"
    );
}

#[test]
fn test_add_and_remove_admin_emit_roster_events() {
    let (env, admin, _, _, _, _, _, dispute_client) = setup_test();
    let new_admin = Address::generate(&env);
    let third_admin = Address::generate(&env);

    dispute_client.add_admin(&admin, &new_admin);
    assert_roster_event(
        &env,
        &dispute_client.address,
        "admin_added",
        &admin,
        &new_admin,
    );

    dispute_client.add_admin(&admin, &third_admin);
    assert_roster_event(
        &env,
        &dispute_client.address,
        "admin_added",
        &admin,
        &third_admin,
    );

    // Roster is [admin, new_admin, third_admin]; removing third_admin still
    // leaves two admins, so the consolidation guard does not fire.
    dispute_client.remove_admin(&new_admin, &third_admin);
    assert_roster_event(
        &env,
        &dispute_client.address,
        "admin_removed",
        &new_admin,
        &third_admin,
    );
}

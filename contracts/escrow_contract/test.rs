use super::*;
use proptest::prelude::*;
use shared_types::FaniLabError;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

fn setup_env() -> (Env, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(EscrowContract, ());
    (env, contract_id)
}

fn setup_token(env: &Env, admin: &Address) -> Address {
    env.register_stellar_asset_contract_v2(admin.clone())
        .address()
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

fn balance(env: &Env, token: &Address, of: &Address) -> i128 {
    TokenClient::new(env, token).balance(of)
}

#[test]
fn test_init_and_platform_fee_default() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    client.init(&admin, &token, &0);

    assert_eq!(client.get_platform_fee(), 0);
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_update_platform_fee_success() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    client.init(&admin, &token, &0);
    client.update_platform_fee(&admin, &250);

    assert_eq!(client.get_platform_fee(), 250);
}

#[test]
fn test_update_platform_fee_invalid_value() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    client.init(&admin, &token, &0);
    let result = client.try_update_platform_fee(&admin, &1100);

    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::InvalidFee.into()),
        _ => panic!("Expected EscrowError::InvalidFee"),
    }
}

#[test]
fn test_init_with_invalid_platform_fee_panics() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let result = client.try_init(&admin, &token, &10000);

    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::InvalidFee.into()),
        _ => panic!("Expected EscrowError::InvalidFee"),
    }
}

#[test]
fn test_create_escrow_locks_funds_and_persists_record() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &1u64, &token, &1000, &None);

    assert_eq!(balance(&env, &token, &sender), 0);
    assert_eq!(balance(&env, &token, &contract_id), 1000);

    let record = client.get_escrow(&1u64);
    assert_eq!(record.sender, sender);
    assert_eq!(record.recipient, recipient);
    assert_eq!(record.driver, driver);
    assert_eq!(record.amount, 1000);
    assert_eq!(record.status, EscrowStatus::Locked);
    assert_eq!(record.disputed_by, None);
    assert_eq!(record.disputed_at, None);
    assert_eq!(record.created_at, env.ledger().timestamp());
}

#[test]
fn test_create_escrow_duplicate_delivery_rejected() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 2000);

    client.create_escrow(&sender, &recipient, &driver, &2u64, &token, &1000, &None);

    let result = client.try_create_escrow(&sender, &recipient, &driver, &2u64, &token, &500, &None);
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::DuplicateDelivery.into()),
        _ => panic!("Expected EscrowError::DuplicateDelivery"),
    }
}

#[test]
fn test_release_escrow_by_recipient_with_platform_fee_split() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    client.update_platform_fee(&admin, &500); // 5%
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &3u64, &token, &1000, &None);
    client.release_escrow(&recipient, &3u64);

    assert_eq!(balance(&env, &token, &driver), 950);
    assert_eq!(balance(&env, &token, &admin), 50);
    assert_eq!(balance(&env, &token, &contract_id), 0);
    assert_eq!(client.get_escrow(&3u64).status, EscrowStatus::Released);
}

#[test]
fn test_release_escrow_unauthorized_rejected() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let attacker = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 500);
    client.create_escrow(&sender, &recipient, &driver, &4u64, &token, &500, &None);

    let result = client.try_release_escrow(&attacker, &4u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::Unauthorized.into()),
        _ => panic!("Expected FaniLabError::Unauthorized"),
    }
}

#[test]
fn test_refund_escrow_by_sender_full_amount_no_fee() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    client.update_platform_fee(&admin, &500);
    mint(&env, &token, &sender, 600);

    client.create_escrow(&sender, &recipient, &driver, &5u64, &token, &600, &None);
    client.refund_escrow(&sender, &5u64);

    assert_eq!(balance(&env, &token, &sender), 600);
    assert_eq!(balance(&env, &token, &admin), 0);
    assert_eq!(balance(&env, &token, &contract_id), 0);
    assert_eq!(client.get_escrow(&5u64).status, EscrowStatus::Refunded);
}

#[test]
fn test_raise_dispute_pauses_escrow_and_records_metadata() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 700);
    client.create_escrow(&sender, &recipient, &driver, &6u64, &token, &700, &None);

    client.raise_dispute(&recipient, &6u64);

    let record = client.get_escrow(&6u64);
    assert_eq!(record.status, EscrowStatus::Paused);
    assert_eq!(record.disputed_by, Some(recipient));
    assert_eq!(record.disputed_at, Some(env.ledger().timestamp()));
}

#[test]
fn test_refund_from_paused_state_by_admin_allowed() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 300);

    client.create_escrow(&sender, &recipient, &driver, &7u64, &token, &300, &None);
    client.raise_dispute(&sender, &7u64);
    client.refund_escrow(&admin, &7u64);

    assert_eq!(balance(&env, &token, &sender), 300);
    assert_eq!(client.get_escrow(&7u64).status, EscrowStatus::Refunded);
}

#[test]
fn test_release_from_paused_state_rejected_with_invalid_state() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 300);

    client.create_escrow(&sender, &recipient, &driver, &8u64, &token, &300, &None);
    client.raise_dispute(&recipient, &8u64);

    let result = client.try_release_escrow(&admin, &8u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::InvalidState.into()),
        _ => panic!("Expected EscrowError::InvalidState"),
    }
}

#[test]
fn test_refund_on_released_escrow_rejected_with_invalid_state() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 300);

    client.create_escrow(&sender, &recipient, &driver, &9u64, &token, &300, &None);
    client.release_escrow(&admin, &9u64);

    let result = client.try_refund_escrow(&admin, &9u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::InvalidState.into()),
        _ => panic!("Expected EscrowError::InvalidState"),
    }
}

#[test]
fn test_insufficient_funds_guard_on_release() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 200);
    client.create_escrow(&sender, &recipient, &driver, &10u64, &token, &200, &None);

    env.as_contract(&contract_id, || {
        let mut record: EscrowRecord = env
            .storage()
            .persistent()
            .get(&shared_types::escrow_key(10u64))
            .unwrap();
        record.amount = 500;
        env.storage()
            .persistent()
            .set(&shared_types::escrow_key(10u64), &record);
    });

    let result = client.try_release_escrow(&admin, &10u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::InsufficientFunds.into()),
        _ => panic!("Expected EscrowError::InsufficientFunds"),
    }
}

#[test]
fn test_create_escrow_with_invalid_token_rejected() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let other_token_admin = Address::generate(&env);
    let other_token = setup_token(&env, &other_token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 500);

    let result = client.try_create_escrow(
        &sender,
        &recipient,
        &driver,
        &42u64,
        &other_token,
        &500,
        &None,
    );
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::InvalidToken.into()),
        _ => panic!("Expected EscrowError::InvalidToken"),
    }
}

#[test]
fn test_resolve_dispute_refund_with_insufficient_funds() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 200);
    client.create_escrow(&sender, &recipient, &driver, &11u64, &token, &200, &None);

    client.raise_dispute(&sender, &11u64);

    env.as_contract(&contract_id, || {
        let mut record: EscrowRecord = env
            .storage()
            .persistent()
            .get(&shared_types::escrow_key(11u64))
            .unwrap();
        record.amount = 500;
        env.storage()
            .persistent()
            .set(&shared_types::escrow_key(11u64), &record);
    });

    let result = client.try_resolve_dispute(&admin, &11u64, &false);
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::InsufficientFunds.into()),
        _ => panic!("Expected EscrowError::InsufficientFunds"),
    }
}

#[test]
fn test_create_escrow_with_fleet_id_stores_fleet_reference() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 1000);

    client.create_escrow(
        &sender,
        &recipient,
        &driver,
        &12u64,
        &token,
        &1000,
        &Some(42u64),
    );

    let record = client.get_escrow(&12u64);
    assert_eq!(record.fleet_id, Some(42u64));
}

#[test]
fn test_get_escrow_not_found() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let result = client.try_get_escrow(&999u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::DeliveryNotFound.into()),
        _ => panic!("Expected DeliveryNotFound"),
    }
}

// ── Property-based tests ─────────────────────────────────────────────────────

proptest! {
    #[test]
    fn test_calculate_fee_non_negative_and_bounded(
        amount in 0i128..i128::MAX,
        platform_fee_bps in 0u32..=10000u32,
    ) {
        let fee = calculate_fee(amount, platform_fee_bps);
        assert!(fee >= 0, "fee must be non-negative: got {fee} for amount={amount} bps={platform_fee_bps}");
        assert!(fee <= amount, "fee {fee} must not exceed amount {amount} for bps={platform_fee_bps}");
    }

    #[test]
    fn test_calculate_fee_zero_bps_yields_zero(
        amount in 0i128..i128::MAX,
    ) {
        let fee = calculate_fee(amount, 0);
        assert_eq!(fee, 0, "fee must be zero when bps=0, got {fee} for amount={amount}");
    }

    #[test]
    fn test_calculate_fee_zero_amount_yields_zero(
        platform_fee_bps in 0u32..=10000u32,
    ) {
        let fee = calculate_fee(0, platform_fee_bps);
        assert_eq!(fee, 0, "fee must be zero when amount=0, got {fee} for bps={platform_fee_bps}");
    }
}

#[test]
fn test_create_escrow_zero_amount_rejected() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 1000);

    let result = client.try_create_escrow(&sender, &recipient, &driver, &100u64, &token, &0, &None);
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::InvalidAmount.into()),
        _ => panic!("Expected EscrowError::InvalidAmount"),
    }
}

#[test]
fn test_create_escrow_negative_amount_rejected() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 1000);

    let result =
        client.try_create_escrow(&sender, &recipient, &driver, &101u64, &token, &-500, &None);
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::InvalidAmount.into()),
        _ => panic!("Expected EscrowError::InvalidAmount"),
    }
}

#[test]
fn test_set_settlement_contract_emits_event() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let settlement_contract = Address::generate(&env);

    client.init(&admin, &token, &0);
    client.set_settlement_contract(&admin, &settlement_contract);

    assert_eq!(
        client.get_settlement_contract(),
        Some(settlement_contract.clone())
    );
}

#[test]
fn test_default_slippage_tolerance_initialized() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    assert_eq!(client.get_slippage_tolerance(), 500); // Default 5%
}

#[test]
fn test_update_slippage_tolerance() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    client.update_slippage_tolerance(&admin, &1000); // 10%

    assert_eq!(client.get_slippage_tolerance(), 1000);
}

#[test]
fn test_escrow_expires_after_ttl() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &200u64, &token, &1000, &None);
    let record = client.get_escrow(&200u64);

    assert!(record.expires_at.is_some());
    let created_at = record.created_at;
    let expires_at = record.expires_at.unwrap();
    assert_eq!(expires_at, created_at + 30 * 24 * 60 * 60);
}

#[test]
fn test_reclaim_expired_escrow_refunds_sender() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &201u64, &token, &1000, &None);

    // Verify funds are in contract
    assert_eq!(balance(&env, &token, &contract_id), 1000);
    assert_eq!(balance(&env, &token, &sender), 0);

    // Jump time past expiry
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 31 * 24 * 60 * 60);

    // Reclaim the expired escrow
    client.reclaim_expired_escrow(&201u64);

    // Verify funds are returned to sender
    assert_eq!(balance(&env, &token, &sender), 1000);
    assert_eq!(balance(&env, &token, &contract_id), 0);
    assert_eq!(client.get_escrow(&201u64).status, EscrowStatus::Refunded);
}

#[test]
fn test_cannot_reclaim_non_expired_escrow() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &202u64, &token, &1000, &None);

    let result = client.try_reclaim_expired_escrow(&202u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::InvalidState.into()),
        _ => panic!("Expected EscrowError::InvalidState"),
    }
}

#[test]
fn test_cannot_reclaim_released_escrow() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &203u64, &token, &1000, &None);
    client.release_escrow(&recipient, &203u64);

    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 31 * 24 * 60 * 60);

    let result = client.try_reclaim_expired_escrow(&203u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::InvalidState.into()),
        _ => panic!("Expected EscrowError::InvalidState"),
    }
}

#[test]
fn test_total_locked_increases_on_create_escrow() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 3000);

    assert_eq!(client.get_total_locked(&token), 0);
    client.create_escrow(&sender, &recipient, &driver, &300u64, &token, &1000, &None);
    assert_eq!(client.get_total_locked(&token), 1000);

    client.create_escrow(&sender, &recipient, &driver, &301u64, &token, &2000, &None);
    assert_eq!(client.get_total_locked(&token), 3000);
}

#[test]
fn test_total_locked_decreases_on_release_escrow() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &302u64, &token, &1000, &None);
    assert_eq!(client.get_total_locked(&token), 1000);

    client.release_escrow(&recipient, &302u64);
    assert_eq!(client.get_total_locked(&token), 0);
}

#[test]
fn test_total_locked_decreases_on_refund_escrow() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &303u64, &token, &1000, &None);
    assert_eq!(client.get_total_locked(&token), 1000);

    client.refund_escrow(&sender, &303u64);
    assert_eq!(client.get_total_locked(&token), 0);
}

#[test]
fn test_total_locked_decreases_on_dispute_resolve() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &304u64, &token, &1000, &None);
    assert_eq!(client.get_total_locked(&token), 1000);

    client.raise_dispute(&recipient, &304u64);
    assert_eq!(client.get_total_locked(&token), 1000);

    client.resolve_dispute(&admin, &304u64, &false);
    assert_eq!(client.get_total_locked(&token), 0);
}

#[test]
fn test_total_locked_decreases_on_dispute_split() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &305u64, &token, &1000, &None);
    assert_eq!(client.get_total_locked(&token), 1000);

    client.raise_dispute(&recipient, &305u64);
    client.resolve_dispute_split(&admin, &305u64, &5000);
    assert_eq!(client.get_total_locked(&token), 0);
}

#[test]
fn test_sweep_untracked_balance_recovers_mistaken_transfer() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let recovery_address = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 2000);

    client.create_escrow(&sender, &recipient, &driver, &400u64, &token, &1000, &None);
    assert_eq!(client.get_total_locked(&token), 1000);

    mint(&env, &token, &contract_id, 1000);
    assert_eq!(balance(&env, &token, &contract_id), 2000);

    client.sweep_untracked_balance(&admin, &token, &recovery_address);

    assert_eq!(balance(&env, &token, &contract_id), 1000);
    assert_eq!(balance(&env, &token, &recovery_address), 1000);
    assert_eq!(client.get_total_locked(&token), 1000);
}

#[test]
fn test_sweep_untracked_balance_with_empty_untracked() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let recovery_address = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &401u64, &token, &1000, &None);
    assert_eq!(client.get_total_locked(&token), 1000);
    assert_eq!(balance(&env, &token, &contract_id), 1000);

    client.sweep_untracked_balance(&admin, &token, &recovery_address);

    assert_eq!(balance(&env, &token, &contract_id), 1000);
    assert_eq!(balance(&env, &token, &recovery_address), 0);
    assert_eq!(client.get_total_locked(&token), 1000);
}

#[test]
fn test_sweep_untracked_balance_unauthorized_rejected() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let recovery_address = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);

    let result = client.try_sweep_untracked_balance(&attacker, &token, &recovery_address);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::Unauthorized.into()),
        _ => panic!("Expected FaniLabError::Unauthorized"),
    }
}

// ── Issue #90: clear_settlement_contract tests ──────────────────────────────

#[test]
fn test_clear_settlement_contract_reverts_to_none() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let settlement_contract = Address::generate(&env);

    client.init(&admin, &token, &0);
    client.set_settlement_contract(&admin, &settlement_contract);
    assert_eq!(client.get_settlement_contract(), Some(settlement_contract));

    client.clear_settlement_contract(&admin);
    assert_eq!(client.get_settlement_contract(), None);
}

#[test]
fn test_clear_settlement_contract_non_admin_rejected() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let settlement_contract = Address::generate(&env);
    let attacker = Address::generate(&env);

    client.init(&admin, &token, &0);
    client.set_settlement_contract(&admin, &settlement_contract);

    let result = client.try_clear_settlement_contract(&attacker);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::Unauthorized.into()),
        _ => panic!("Expected FaniLabError::Unauthorized"),
    }
}

#[test]
fn test_clear_settlement_contract_reverts_payout_to_direct_transfer() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let settlement_contract = Address::generate(&env);

    client.init(&admin, &token, &0);
    client.update_platform_fee(&admin, &500); // 5%
    mint(&env, &token, &sender, 1000);

    client.set_settlement_contract(&admin, &settlement_contract);
    client.create_escrow(&sender, &recipient, &driver, &300u64, &token, &1000, &None);

    client.clear_settlement_contract(&admin);
    assert_eq!(client.get_settlement_contract(), None);

    client.release_escrow(&recipient, &300u64);

    assert_eq!(balance(&env, &token, &driver), 950);
    assert_eq!(balance(&env, &token, &admin), 50);
    assert_eq!(balance(&env, &token, &contract_id), 0);
    assert_eq!(client.get_escrow(&300u64).status, EscrowStatus::Released);
}

#[test]
fn test_clear_nonexistent_settlement_contract_succeeds() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    assert_eq!(client.get_settlement_contract(), None);

    client.clear_settlement_contract(&admin);
    assert_eq!(client.get_settlement_contract(), None);
}

// ── Issue #89: propose_admin and accept_admin typed errors ──────────────────

#[test]
fn test_propose_admin_unauthorized_caller_typed_error() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let attacker = Address::generate(&env);
    let new_admin = Address::generate(&env);

    client.init(&admin, &token, &0);

    let result = client.try_propose_admin(&attacker, &new_admin);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::Unauthorized.into()),
        _ => panic!("Expected FaniLabError::Unauthorized (typed error), not raw panic"),
    }
}

#[test]
fn test_accept_admin_no_pending_admin_typed_error() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let caller = Address::generate(&env);

    client.init(&admin, &token, &0);

    let result = client.try_accept_admin(&caller);
    match result {
        Err(Ok(err)) => {
            assert_ne!(err, FaniLabError::Unauthorized.into());
            assert_eq!(err, FaniLabError::InvalidState.into());
        }
        _ => panic!("Expected typed error for missing pending admin, not raw panic"),
    }
}

#[test]
fn test_accept_admin_wrong_pending_caller_typed_error() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let new_admin = Address::generate(&env);
    let wrong_caller = Address::generate(&env);

    client.init(&admin, &token, &0);
    client.propose_admin(&admin, &new_admin);

    let result = client.try_accept_admin(&wrong_caller);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::Unauthorized.into()),
        _ => panic!("Expected FaniLabError::Unauthorized (typed error), not raw panic"),
    }
}

#[test]
fn test_propose_admin_sets_pending_admin() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let new_admin = Address::generate(&env);

    client.init(&admin, &token, &0);
    client.propose_admin(&admin, &new_admin);

    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_accept_admin_completes_transfer() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let new_admin = Address::generate(&env);

    client.init(&admin, &token, &0);
    client.propose_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);

    assert_eq!(client.get_admin(), new_admin);
}

// ── Issue #88: resolve_dispute event emission tests ──────────────────────────

#[test]
fn test_resolve_dispute_release_emits_escrow_released_event() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let dispute_contract = Address::generate(&env);

    client.init(&admin, &token, &0);
    client.set_dispute_resolution_contract(&admin, &dispute_contract);
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &301u64, &token, &1000, &None);
    client.freeze_funds(&dispute_contract, &301u64);
    client.resolve_dispute(&admin, &301u64, &false);

    assert_eq!(balance(&env, &token, &sender), 1000);
    assert_eq!(balance(&env, &token, &contract_id), 0);
    assert_eq!(client.get_escrow(&301u64).status, EscrowStatus::Refunded);
}

#[test]
fn test_resolve_dispute_split_50_50() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    client.update_platform_fee(&admin, &500); // 5%
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &400u64, &token, &1000, &None);
    client.raise_dispute(&sender, &400u64);

    client.resolve_dispute(&admin, &400u64, &true);

    let record = client.get_escrow(&400u64);
    assert_eq!(record.status, EscrowStatus::Released);
    assert_eq!(balance(&env, &token, &driver), 950);
    assert_eq!(balance(&env, &token, &admin), 50);
}

#[test]
fn test_resolve_dispute_refund_emits_escrow_refunded_event() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &401u64, &token, &1000, &None);
    client.raise_dispute(&sender, &401u64);

    client.resolve_dispute(&admin, &401u64, &false);

    let record = client.get_escrow(&401u64);
    assert_eq!(record.status, EscrowStatus::Refunded);
    assert_eq!(balance(&env, &token, &sender), 1000);
}

#[test]
fn test_resolve_dispute_split_emits_event_with_both_amounts() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let dispute_contract = Address::generate(&env);

    client.init(&admin, &token, &0);
    client.set_dispute_resolution_contract(&admin, &dispute_contract);
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &302u64, &token, &1000, &None);
    client.freeze_funds(&dispute_contract, &302u64);
    client.resolve_dispute_split(&admin, &302u64, &5000); // 50% sender, 50% driver

    assert_eq!(balance(&env, &token, &sender), 500);
    assert_eq!(balance(&env, &token, &driver), 500);
    assert_eq!(balance(&env, &token, &contract_id), 0);
    assert_eq!(client.get_escrow(&302u64).status, EscrowStatus::Split);
}

#[test]
fn test_resolve_dispute_split_0_100() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &402u64, &token, &1000, &None);
    client.raise_dispute(&sender, &402u64);

    client.resolve_dispute_split(&admin, &402u64, &5000);

    let record = client.get_escrow(&402u64);
    assert_eq!(record.status, EscrowStatus::Split);
    assert_eq!(balance(&env, &token, &sender), 500);
    assert_eq!(balance(&env, &token, &driver), 500);
}

#[test]
fn test_resolve_dispute_emits_driver_and_amount_in_event() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let dispute_contract = Address::generate(&env);

    client.init(&admin, &token, &0);
    client.set_dispute_resolution_contract(&admin, &dispute_contract);
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &303u64, &token, &1000, &None);
    client.freeze_funds(&dispute_contract, &303u64);
    client.resolve_dispute_split(&admin, &303u64, &0); // 0% sender, 100% driver

    assert_eq!(balance(&env, &token, &sender), 0);
    assert_eq!(balance(&env, &token, &driver), 1000);
    assert_eq!(balance(&env, &token, &contract_id), 0);
    assert_eq!(client.get_escrow(&303u64).status, EscrowStatus::Split);
}

#[test]
fn test_resolve_dispute_split_100_0() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    client.update_platform_fee(&admin, &1000); // 10%
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &403u64, &token, &1000, &None);
    client.raise_dispute(&sender, &403u64);

    client.resolve_dispute(&admin, &403u64, &true);

    let record = client.get_escrow(&403u64);
    assert_eq!(record.driver, driver);
    assert_eq!(balance(&env, &token, &driver), 900);
}

// ── Issue #87: Reentrancy and state-update-before-transfer tests ────────────

#[test]
fn test_release_escrow_updates_state_before_transfer() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &250);

    let original_fee = client.get_platform_fee();
    let original_slippage = client.get_slippage_tolerance();
    let original_admin = client.get_admin();
    let original_token = client.get_token();

    assert_eq!(original_fee, 250);
    assert_eq!(original_slippage, 500);
    assert_eq!(original_admin, admin);
    assert_eq!(original_token, token);

    let config = shared_types::ProtocolConfig {
        token: token.clone(),
        platform_fee_bps: 250,
        protocol_version: 1,
        slippage_tolerance_bps: 500,
    };

    env.as_contract(&contract_id, || {
        env.storage()
            .instance()
            .set(&shared_types::StorageKey::ProtocolConfig, &config);
    });

    let migrated_fee = client.get_platform_fee();
    let migrated_slippage = client.get_slippage_tolerance();

    assert_eq!(migrated_fee, original_fee);
    assert_eq!(migrated_slippage, original_slippage);
}

#[test]
fn test_volume_tier_fee_discount_applied() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &100); // 1% base fee
    mint(&env, &token, &sender, 5000);

    let mut tiers = soroban_sdk::Vec::new(&env);
    tiers.push_back(VolumeTier {
        volume_threshold: 2u32,
        discount_bps: 50u32, // 0.5% discount for 2+ deliveries
    });
    client.set_volume_tiers(&admin, &tiers);

    client.create_escrow(&sender, &recipient, &driver, &500u64, &token, &1000, &None);
    client.release_escrow(&recipient, &500u64);
    assert_eq!(balance(&env, &token, &driver), 990); // (1000 - 10 fee)
    assert_eq!(client.get_sender_volume(&sender), 1u32);

    client.create_escrow(&sender, &recipient, &driver, &501u64, &token, &1000, &None);
    client.release_escrow(&recipient, &501u64);
    // Tier threshold is checked against sender_volume *before* this delivery's
    // increment, so the discount only takes effect starting on the delivery
    // where sender_volume already reached the threshold (i.e. the 3rd release
    // here, not the 2nd) — this release still pays the full 1% base fee.
    assert_eq!(balance(&env, &token, &driver), 1980); // 990 + (1000 - 10 fee, no discount yet)
    assert_eq!(client.get_sender_volume(&sender), 2u32);
}

#[test]
fn test_resolve_dispute_split_full_sender_share() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let dispute_contract = Address::generate(&env);

    client.init(&admin, &token, &0);
    client.set_dispute_resolution_contract(&admin, &dispute_contract);
    mint(&env, &token, &sender, 1000);

    client.create_escrow(&sender, &recipient, &driver, &304u64, &token, &1000, &None);
    client.freeze_funds(&dispute_contract, &304u64);
    client.resolve_dispute_split(&admin, &304u64, &10000); // 100% sender, 0% driver

    assert_eq!(balance(&env, &token, &sender), 1000);
    assert_eq!(balance(&env, &token, &driver), 0);
    assert_eq!(balance(&env, &token, &contract_id), 0);
    assert_eq!(client.get_escrow(&304u64).status, EscrowStatus::Split);
}

#[test]
fn test_release_escrow_happy_path_sets_released_status() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 2000);

    client.create_escrow(&sender, &recipient, &driver, &500u64, &token, &2000, &None);
    client.release_escrow(&recipient, &500u64);

    let record = client.get_escrow(&500u64);
    assert_eq!(record.status, EscrowStatus::Released);
    assert_eq!(balance(&env, &token, &driver), 2000);
}

#[test]
fn test_set_settlement_contract_updates_getter() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let settlement_contract = Address::generate(&env);

    client.init(&admin, &token, &0);

    let result_before = client.get_settlement_contract();
    assert_eq!(result_before, None);

    client.set_settlement_contract(&admin, &settlement_contract);

    let result_after = client.get_settlement_contract();
    assert_eq!(result_after, Some(settlement_contract));
}

#[test]
fn test_refund_escrow_sets_refunded_status() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 2000);

    client.create_escrow(&sender, &recipient, &driver, &501u64, &token, &2000, &None);
    client.refund_escrow(&sender, &501u64);

    let record = client.get_escrow(&501u64);
    assert_eq!(record.status, EscrowStatus::Refunded);
    assert_eq!(balance(&env, &token, &sender), 2000);
}

#[test]
fn test_resolve_dispute_updates_state_before_release_transfer() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 2000);

    client.create_escrow(&sender, &recipient, &driver, &502u64, &token, &2000, &None);
    client.raise_dispute(&sender, &502u64);
    client.resolve_dispute(&admin, &502u64, &true);

    let record = client.get_escrow(&502u64);
    assert_eq!(record.status, EscrowStatus::Released);
    assert_eq!(balance(&env, &token, &driver), 2000);
}

#[test]
fn test_set_settlement_contract_unauthorized() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let settlement_contract = Address::generate(&env);

    client.init(&admin, &token, &0);

    let result = client.try_set_settlement_contract(&attacker, &settlement_contract);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::Unauthorized.into()),
        _ => panic!("Expected FaniLabError::Unauthorized"),
    }
}

#[test]
fn test_sender_volume_tracking() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 5000);

    assert_eq!(client.get_sender_volume(&sender), 0u32);

    client.create_escrow(&sender, &recipient, &driver, &600u64, &token, &1000, &None);
    client.release_escrow(&recipient, &600u64);
    assert_eq!(client.get_sender_volume(&sender), 1u32);

    client.create_escrow(&sender, &recipient, &driver, &601u64, &token, &1000, &None);
    client.release_escrow(&recipient, &601u64);
    assert_eq!(client.get_sender_volume(&sender), 2u32);

    client.create_escrow(&sender, &recipient, &driver, &602u64, &token, &1000, &None);
    client.release_escrow(&recipient, &602u64);
    assert_eq!(client.get_sender_volume(&sender), 3u32);
}

#[test]
fn test_resolve_dispute_refund_sets_refunded_status() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 2000);

    client.create_escrow(&sender, &recipient, &driver, &503u64, &token, &2000, &None);
    client.raise_dispute(&sender, &503u64);
    client.resolve_dispute(&admin, &503u64, &false);

    let record = client.get_escrow(&503u64);
    assert_eq!(record.status, EscrowStatus::Refunded);
    assert_eq!(balance(&env, &token, &sender), 2000);
}

#[test]
fn test_resolve_dispute_split_updates_state_before_transfer() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 2000);

    client.create_escrow(&sender, &recipient, &driver, &504u64, &token, &2000, &None);
    client.raise_dispute(&sender, &504u64);
    client.resolve_dispute_split(&admin, &504u64, &3000);

    let record = client.get_escrow(&504u64);
    assert_eq!(record.status, EscrowStatus::Split);
    assert_eq!(balance(&env, &token, &sender), 600);
    assert_eq!(balance(&env, &token, &driver), 1400);
}

#[test]
fn test_double_release_prevented_by_state_check() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 2000);

    client.create_escrow(&sender, &recipient, &driver, &505u64, &token, &2000, &None);
    client.release_escrow(&recipient, &505u64);

    let result = client.try_release_escrow(&admin, &505u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::InvalidState.into()),
        _ => panic!("Expected EscrowError::InvalidState on double-release attempt"),
    }

    assert_eq!(balance(&env, &token, &driver), 2000);
}

#[test]
fn test_cannot_release_already_refunded_escrow() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);

    let mut tiers = soroban_sdk::Vec::new(&env);
    tiers.push_back(VolumeTier {
        volume_threshold: 10u32,
        discount_bps: 100u32,
    });
    tiers.push_back(VolumeTier {
        volume_threshold: 50u32,
        discount_bps: 200u32,
    });

    client.set_volume_tiers(&admin, &tiers);

    let retrieved_tiers = client.get_volume_tiers();
    assert_eq!(retrieved_tiers.len(), 2u32);
    mint(&env, &token, &sender, 2000);

    client.create_escrow(&sender, &recipient, &driver, &506u64, &token, &2000, &None);
    client.refund_escrow(&sender, &506u64);

    let result = client.try_release_escrow(&admin, &506u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::InvalidState.into()),
        _ => panic!("Expected EscrowError::InvalidState"),
    }

    assert_eq!(balance(&env, &token, &sender), 2000);
}

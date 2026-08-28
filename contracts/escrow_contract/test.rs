use super::*;
use proptest::prelude::*;
use shared_types::FaniLabError;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

proptest! {
    #[test]
    fn split_conserves_funds(amount in 0i128..i128::MAX, bps in 0u32..=10_000) {
        let sender = amount.saturating_mul(bps as i128) / 10_000;
        prop_assert_eq!(sender + amount.saturating_sub(sender), amount);
    }

    #[test]
    fn effective_fee_never_exceeds_base(base in 0u32..=10_000, volume in any::<u32>()) {
        let env = Env::default();
        prop_assert!(get_effective_fee_bps(&env, base, volume) <= base);
    }
}

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

/// A malicious settlement_contract used to prove the Issue #87
/// checks-effects-interactions fix: its `execute_settlement_swap` re-enters
/// `release_escrow` on the same delivery mid-payout, before the outer call
/// would otherwise have returned.
#[contract]
struct MaliciousSettlementContract;

#[contractimpl]
impl MaliciousSettlementContract {
    pub fn get_driver_preference(env: Env, _driver: Address) -> Option<Address> {
        // Any address different from the escrow's real token forces
        // payout_driver into the execute_settlement_swap path.
        Some(Address::generate(&env))
    }

    pub fn execute_settlement_swap(
        env: Env,
        _caller: Address,
        _from_token: Address,
        _to_token: Address,
        _recipient: Address,
        _amount: i128,
        _min_amount_out: i128,
    ) {
        let target: Address = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "target"))
            .unwrap();
        let _: () = env.invoke_contract(
            &target,
            &Symbol::new(&env, "release_escrow"),
            soroban_sdk::vec![&env, _recipient.into_val(&env), 900u64.into_val(&env)],
        );
    }
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

/// Regression test for Issue #93 (FA-2): before this fix, a sender could
/// raise a dispute and then immediately self-refund via refund_escrow,
/// bypassing admin/dispute_resolution_contract entirely. Only an admin may
/// now refund a Paused (disputed) escrow.
#[test]
fn test_sender_cannot_self_refund_disputed_escrow() {
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

    client.create_escrow(&sender, &recipient, &driver, &910u64, &token, &300, &None);
    client.raise_dispute(&sender, &910u64);

    let result = client.try_refund_escrow(&sender, &910u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::Unauthorized.into()),
        _ => panic!("Expected FaniLabError::Unauthorized"),
    }

    // Funds must still be locked in the contract, untouched.
    assert_eq!(balance(&env, &token, &sender), 0);
    assert_eq!(client.get_escrow(&910u64).status, EscrowStatus::Paused);
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

    // set_settlement_contract only proposes the change (Issue #16 timelock);
    // it must be confirmed after the timelock elapses to actually apply.
    assert_eq!(client.get_settlement_contract(), None);
    let pending = client.get_pending_settlement_contract().unwrap();
    assert_eq!(pending.settlement_contract, settlement_contract);

    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 3 * 24 * 60 * 60);
    client.confirm_settlement_contract(&admin);

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
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 3 * 24 * 60 * 60);
    client.confirm_settlement_contract(&admin);
    assert_eq!(client.get_settlement_contract(), Some(settlement_contract));

    client.clear_settlement_contract(&admin);
    assert_eq!(client.get_settlement_contract(), None);
}

#[test]
fn test_clear_settlement_contract_also_cancels_pending_proposal() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let settlement_contract = Address::generate(&env);

    client.init(&admin, &token, &0);
    client.set_settlement_contract(&admin, &settlement_contract);
    assert!(client.get_pending_settlement_contract().is_some());

    client.clear_settlement_contract(&admin);
    assert_eq!(client.get_pending_settlement_contract(), None);

    // The now-cancelled proposal can no longer be confirmed.
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 3 * 24 * 60 * 60);
    let result = client.try_confirm_settlement_contract(&admin);
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::NoPendingSettlementChange.into()),
        _ => panic!("Expected EscrowError::NoPendingSettlementChange"),
    }
}

#[test]
fn test_confirm_settlement_contract_before_timelock_rejected() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let settlement_contract = Address::generate(&env);

    client.init(&admin, &token, &0);
    client.set_settlement_contract(&admin, &settlement_contract);

    let result = client.try_confirm_settlement_contract(&admin);
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::TimelockNotElapsed.into()),
        _ => panic!("Expected EscrowError::TimelockNotElapsed"),
    }
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
fn test_protocol_config_direct_storage_write_is_readable_by_getters() {
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

// ── Issue #87: checks-effects-interactions reentrancy regression ────────────

#[test]
#[should_panic(expected = "Contract re-entry is not allowed")]
fn test_release_escrow_rejects_reentrant_call_during_settlement_swap() {
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
    client.create_escrow(&sender, &recipient, &driver, &900u64, &token, &1000, &None);

    // A malicious settlement_contract whose get_driver_preference forces the
    // execute_settlement_swap path, from which it re-enters release_escrow
    // on the same delivery before the outer call would have returned.
    // Soroban's host itself blocks same-contract reentrancy ("Contract
    // re-entry is not allowed"), so this is defense-in-depth on top of a
    // platform-level guarantee, not the last line of defense: the
    // checks-effects-interactions ordering fixed for Issue #87 still
    // matters because it also determines what state a *legitimate*
    // cross-contract call (e.g. a real DEX during execute_settlement_swap)
    // would observe if it queried get_escrow mid-payout.
    let malicious_id = env.register(MaliciousSettlementContract, ());
    env.as_contract(&malicious_id, || {
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "target"), &contract_id);
    });
    client.set_settlement_contract(&admin, &malicious_id);
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 3 * 24 * 60 * 60);
    client.confirm_settlement_contract(&admin);

    client.release_escrow(&recipient, &900u64);
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

    // Proposing alone must not update the active getter until confirmed.
    assert_eq!(client.get_settlement_contract(), None);

    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 3 * 24 * 60 * 60);
    client.confirm_settlement_contract(&admin);

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

// ── Batch FB-3: Emergency pause / circuit breaker (Issue #31) ───────────────

#[test]
fn test_set_paused_requires_admin() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let not_admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);

    let result = client.try_set_paused(&not_admin, &true);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::Unauthorized.into()),
        _ => panic!("Expected FaniLabError::Unauthorized"),
    }
    assert!(!client.is_paused());
}

#[test]
fn test_set_paused_and_is_paused_roundtrip() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    assert!(!client.is_paused());

    client.set_paused(&admin, &true);
    assert!(client.is_paused());

    client.set_paused(&admin, &false);
    assert!(!client.is_paused());
}

/// Shared fixture: an initialized, paused protocol with one funded, Locked
/// escrow — enough starting state for every paused-rejection test below,
/// since `require_not_paused` fires before any function's own state or
/// authorization checks.
fn setup_paused_with_escrow(
    delivery_id: u64,
) -> (Env, Address, Address, Address, Address, Address) {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 10_000);
    client.create_escrow(
        &sender,
        &recipient,
        &driver,
        &delivery_id,
        &token,
        &1000,
        &None,
    );
    client.set_paused(&admin, &true);

    (env, contract_id, admin, sender, recipient, driver)
}

#[test]
fn test_create_escrow_rejected_while_paused() {
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
    client.set_paused(&admin, &true);

    let result =
        client.try_create_escrow(&sender, &recipient, &driver, &900u64, &token, &1000, &None);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::ProtocolPaused.into()),
        _ => panic!("Expected FaniLabError::ProtocolPaused"),
    }
}

#[test]
fn test_create_escrows_batch_rejected_while_paused() {
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
    client.set_paused(&admin, &true);

    let mut escrow_list = soroban_sdk::Vec::new(&env);
    escrow_list.push_back((901u64, driver, 500i128));

    let result = client.try_create_escrows_batch(&sender, &recipient, &token, &escrow_list);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::ProtocolPaused.into()),
        _ => panic!("Expected FaniLabError::ProtocolPaused"),
    }
}

// ── Issue #188: create_escrows_batch must maintain TotalLocked ──────────────

#[test]
fn test_batch_increases_total_locked() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 6000);

    let mut escrow_list = soroban_sdk::Vec::new(&env);
    escrow_list.push_back((1u64, driver.clone(), 1000i128));
    escrow_list.push_back((2u64, driver.clone(), 2000i128));
    escrow_list.push_back((3u64, driver.clone(), 3000i128));

    assert_eq!(client.get_total_locked(&token), 0);
    assert_eq!(
        client.create_escrows_batch(&sender, &recipient, &token, &escrow_list),
        3
    );
    assert_eq!(client.get_total_locked(&token), 6000);
}

#[test]
fn test_batch_release_each_returns_total_locked_to_zero() {
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

    let mut escrow_list = soroban_sdk::Vec::new(&env);
    escrow_list.push_back((4u64, driver.clone(), 1000i128));
    escrow_list.push_back((5u64, driver.clone(), 2000i128));

    client.create_escrows_batch(&sender, &recipient, &token, &escrow_list);
    assert_eq!(client.get_total_locked(&token), 3000);

    client.release_escrow(&recipient, &4u64);
    assert_eq!(client.get_total_locked(&token), 2000);

    client.release_escrow(&recipient, &5u64);
    assert_eq!(client.get_total_locked(&token), 0);
}

#[test]
fn test_sweep_untracked_balance_after_batch_moves_no_funds() {
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

    let mut escrow_list = soroban_sdk::Vec::new(&env);
    escrow_list.push_back((6u64, driver.clone(), 1000i128));
    escrow_list.push_back((7u64, driver.clone(), 1000i128));

    client.create_escrows_batch(&sender, &recipient, &token, &escrow_list);
    assert_eq!(client.get_total_locked(&token), 2000);
    assert_eq!(balance(&env, &token, &contract_id), 2000);

    // Batch-created escrows are fully tracked, so nothing is untracked.
    client.sweep_untracked_balance(&admin, &token, &recovery_address);

    assert_eq!(balance(&env, &token, &contract_id), 2000);
    assert_eq!(balance(&env, &token, &recovery_address), 0);
    assert_eq!(client.get_total_locked(&token), 2000);

    // Every batch-created escrow remains settleable after the sweep.
    client.release_escrow(&recipient, &6u64);
    client.release_escrow(&recipient, &7u64);
    assert_eq!(client.get_total_locked(&token), 0);
}

#[test]
fn test_batch_total_locked_single_and_max_batch_size() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    // A MAX_BATCH_SIZE batch performs 100 cross-contract token transfers plus
    // per-record storage writes, which exceeds both the default test-host CPU
    // budget and the mainnet invocation resource limits (footprint/writes/
    // events). Disable both for this edge-case test only.
    env.cost_estimate().disable_resource_limits();
    env.cost_estimate()
        .budget()
        .reset_limits(1_000_000_000, 1_000_000_000);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 10_000);

    // Edge case: batch of size 1.
    let mut single = soroban_sdk::Vec::new(&env);
    single.push_back((8u64, driver.clone(), 500i128));
    client.create_escrows_batch(&sender, &recipient, &token, &single);
    assert_eq!(client.get_total_locked(&token), 500);

    // Edge case: batch at MAX_BATCH_SIZE.
    let mut max_batch = soroban_sdk::Vec::new(&env);
    for i in 0..constants::MAX_BATCH_SIZE {
        max_batch.push_back((100u64 + u64::from(i), driver.clone(), 10i128));
    }
    client.create_escrows_batch(&sender, &recipient, &token, &max_batch);
    assert_eq!(client.get_total_locked(&token), 500 + 100 * 10);
}

#[test]
fn test_batch_total_locked_accumulates_across_senders() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender1 = Address::generate(&env);
    let sender2 = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender1, 2000);
    mint(&env, &token, &sender2, 500);

    let mut batch1 = soroban_sdk::Vec::new(&env);
    batch1.push_back((200u64, driver.clone(), 1000i128));
    batch1.push_back((201u64, driver.clone(), 1000i128));
    client.create_escrows_batch(&sender1, &recipient, &token, &batch1);
    assert_eq!(client.get_total_locked(&token), 2000);

    let mut batch2 = soroban_sdk::Vec::new(&env);
    batch2.push_back((202u64, driver.clone(), 500i128));
    client.create_escrows_batch(&sender2, &recipient, &token, &batch2);
    assert_eq!(client.get_total_locked(&token), 2500);
}

// ── Issue #189: create_escrows_batch must enforce create_escrow's guards ─────

#[test]
fn test_batch_with_foreign_token_rejected() {
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

    let mut escrow_list = soroban_sdk::Vec::new(&env);
    escrow_list.push_back((300u64, driver.clone(), 500i128));

    let result = client.try_create_escrows_batch(&sender, &recipient, &other_token, &escrow_list);
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::InvalidToken.into()),
        _ => panic!("Expected EscrowError::InvalidToken"),
    }
    assert_eq!(client.get_total_locked(&token), 0);
}

#[test]
fn test_batch_with_zero_amount_rejected() {
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

    let mut escrow_list = soroban_sdk::Vec::new(&env);
    escrow_list.push_back((301u64, driver.clone(), 0i128));

    let result = client.try_create_escrows_batch(&sender, &recipient, &token, &escrow_list);
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::InvalidAmount.into()),
        _ => panic!("Expected EscrowError::InvalidAmount"),
    }
    assert_eq!(client.get_total_locked(&token), 0);
}

#[test]
fn test_batch_with_negative_amount_rejected() {
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

    let mut escrow_list = soroban_sdk::Vec::new(&env);
    escrow_list.push_back((302u64, driver.clone(), -500i128));

    let result = client.try_create_escrows_batch(&sender, &recipient, &token, &escrow_list);
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::InvalidAmount.into()),
        _ => panic!("Expected EscrowError::InvalidAmount"),
    }
    assert_eq!(client.get_total_locked(&token), 0);
}

#[test]
fn test_batch_invalid_element_leaves_no_partial_state() {
    // Invalid element at position 2 of 3: the whole batch must revert with no
    // partial state — no escrows, no index entries, no funds moved, and no
    // TotalLocked change (Soroban rolls back all storage on panic).
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

    let mut escrow_list = soroban_sdk::Vec::new(&env);
    escrow_list.push_back((400u64, driver.clone(), 1000i128));
    escrow_list.push_back((401u64, driver.clone(), 0i128)); // invalid element at position 2
    escrow_list.push_back((402u64, driver.clone(), 1000i128));

    let result = client.try_create_escrows_batch(&sender, &recipient, &token, &escrow_list);
    match result {
        Err(Ok(err)) => assert_eq!(err, EscrowError::InvalidAmount.into()),
        _ => panic!("Expected EscrowError::InvalidAmount"),
    }

    for delivery_id in [400u64, 401u64, 402u64] {
        let result = client.try_get_escrow(&delivery_id);
        match result {
            Err(Ok(err)) => assert_eq!(err, EscrowError::DeliveryNotFound.into()),
            _ => panic!("Expected DeliveryNotFound for delivery {delivery_id}"),
        }
    }
    assert_eq!(client.get_total_locked(&token), 0);
    assert_eq!(balance(&env, &token, &contract_id), 0);
    assert_eq!(client.get_escrows_by_sender(&sender).len(), 0);
    assert_eq!(client.get_escrows_by_recipient(&recipient).len(), 0);
    assert_eq!(client.get_escrows_by_driver(&driver).len(), 0);
}

#[test]
fn test_batch_valid_creates_every_escrow() {
    // Non-regression: an all-valid batch still creates every escrow, updates
    // the secondary indexes, and maintains TotalLocked.
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

    let mut escrow_list = soroban_sdk::Vec::new(&env);
    escrow_list.push_back((500u64, driver.clone(), 1000i128));
    escrow_list.push_back((501u64, driver.clone(), 1000i128));
    escrow_list.push_back((502u64, driver.clone(), 1000i128));

    assert_eq!(
        client.create_escrows_batch(&sender, &recipient, &token, &escrow_list),
        3
    );

    for delivery_id in [500u64, 501u64, 502u64] {
        let record = client.get_escrow(&delivery_id);
        assert_eq!(record.status, EscrowStatus::Locked);
        assert_eq!(record.amount, 1000);
    }
    assert_eq!(client.get_total_locked(&token), 3000);
    assert_eq!(client.get_escrows_by_sender(&sender).len(), 3);
    assert_eq!(client.get_escrows_by_recipient(&recipient).len(), 3);
    assert_eq!(client.get_escrows_by_driver(&driver).len(), 3);
    assert_eq!(balance(&env, &token, &contract_id), 3000);
}

#[test]
fn test_mark_holdback_escrow_rejected_while_paused() {
    let (env, contract_id, _admin, _sender, recipient, _driver) = setup_paused_with_escrow(902);
    let client = EscrowContractClient::new(&env, &contract_id);

    let result = client.try_mark_holdback_escrow(&recipient, &902u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::ProtocolPaused.into()),
        _ => panic!("Expected FaniLabError::ProtocolPaused"),
    }
}

#[test]
fn test_release_holdback_escrow_rejected_while_paused() {
    let (env, contract_id, _admin, _sender, recipient, _driver) = setup_paused_with_escrow(903);
    let client = EscrowContractClient::new(&env, &contract_id);

    let result = client.try_release_holdback_escrow(&recipient, &903u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::ProtocolPaused.into()),
        _ => panic!("Expected FaniLabError::ProtocolPaused"),
    }
}

#[test]
fn test_release_escrow_rejected_while_paused() {
    let (env, contract_id, _admin, _sender, recipient, _driver) = setup_paused_with_escrow(904);
    let client = EscrowContractClient::new(&env, &contract_id);

    let result = client.try_release_escrow(&recipient, &904u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::ProtocolPaused.into()),
        _ => panic!("Expected FaniLabError::ProtocolPaused"),
    }
}

#[test]
fn test_refund_escrow_rejected_while_paused() {
    let (env, contract_id, _admin, sender, _recipient, _driver) = setup_paused_with_escrow(905);
    let client = EscrowContractClient::new(&env, &contract_id);

    let result = client.try_refund_escrow(&sender, &905u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::ProtocolPaused.into()),
        _ => panic!("Expected FaniLabError::ProtocolPaused"),
    }
}

#[test]
fn test_resolve_dispute_rejected_while_paused() {
    let (env, contract_id, admin, _sender, _recipient, _driver) = setup_paused_with_escrow(906);
    let client = EscrowContractClient::new(&env, &contract_id);

    let result = client.try_resolve_dispute(&admin, &906u64, &true);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::ProtocolPaused.into()),
        _ => panic!("Expected FaniLabError::ProtocolPaused"),
    }
}

#[test]
fn test_resolve_dispute_split_rejected_while_paused() {
    let (env, contract_id, admin, _sender, _recipient, _driver) = setup_paused_with_escrow(907);
    let client = EscrowContractClient::new(&env, &contract_id);

    let result = client.try_resolve_dispute_split(&admin, &907u64, &5000u32);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::ProtocolPaused.into()),
        _ => panic!("Expected FaniLabError::ProtocolPaused"),
    }
}

#[test]
fn test_reclaim_expired_escrow_rejected_while_paused() {
    let (env, contract_id, _admin, _sender, _recipient, _driver) = setup_paused_with_escrow(908);
    let client = EscrowContractClient::new(&env, &contract_id);

    // Jump time past expiry so the only remaining rejection reason would be
    // the protocol pause, not "not yet expired".
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 31 * 24 * 60 * 60);

    let result = client.try_reclaim_expired_escrow(&908u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::ProtocolPaused.into()),
        _ => panic!("Expected FaniLabError::ProtocolPaused"),
    }
}

/// Documents the intentional scope decision: freeze_funds only moves an
/// escrow into the Paused (disputed) state and never transfers funds, so it
/// stays available during a protocol pause — an admin-configured
/// dispute_resolution_contract can still freeze a suspicious escrow while
/// the protocol is paused for an unrelated incident.
#[test]
fn test_freeze_funds_remains_available_while_paused() {
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
    client.create_escrow(&sender, &recipient, &driver, &909u64, &token, &1000, &None);
    client.set_paused(&admin, &true);

    client.freeze_funds(&dispute_contract, &909u64);

    assert_eq!(client.get_escrow(&909u64).status, EscrowStatus::Paused);
}

/// Issue #7 regression test: freeze_funds must reject any caller other than
/// the configured dispute_resolution_contract, otherwise any address could
/// unilaterally DoS every in-flight escrow in the protocol.
#[test]
fn test_freeze_funds_unauthorized_caller_rejected() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let attacker = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);
    let dispute_contract = Address::generate(&env);

    client.init(&admin, &token, &0);
    client.set_dispute_resolution_contract(&admin, &dispute_contract);
    mint(&env, &token, &sender, 1000);
    client.create_escrow(&sender, &recipient, &driver, &910u64, &token, &1000, &None);

    let result = client.try_freeze_funds(&attacker, &910u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::Unauthorized.into()),
        _ => panic!("Expected FaniLabError::Unauthorized"),
    }
    assert_eq!(client.get_escrow(&910u64).status, EscrowStatus::Locked);
}

// ── Holdback refund invariant ────────────────────────────────────────────────
//
// `Holdback` is reached only through `mark_holdback_escrow`, which only the
// recipient may call and which `delivery_contract::confirm_delivery` invokes
// when the recipient confirms the goods arrived. At that point the driver has
// performed and has been credited reputation, so the escrow is earmarked for
// them. The security invariant these tests pin down:
//
//   Once an escrow is in `Holdback`, the sender can never unilaterally
//   reclaim it. Only `release_holdback_escrow` (to the driver) or an
//   admin/dispute arbitration outcome may move the funds.
//
// Refunds from `Locked` (pre-confirmation cancellation) are untouched.

/// Wires the real delivery_contract, escrow_contract and
/// identity_reputation_contract together and drives a delivery all the way
/// through recipient confirmation — the only transition that puts an escrow
/// into `Holdback` and credits the driver's reputation.
///
/// Returns `(env, escrow_id, identity_id, token, admin, sender, driver, delivery_id)`
/// with the escrow sitting in `Holdback`.
#[allow(clippy::type_complexity)]
fn setup_confirmed_delivery_in_holdback(
    amount: i128,
) -> (
    Env,
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
    u64,
) {
    let env = Env::default();
    // delivery_contract::create_delivery cross-calls
    // identity_reputation_contract::register_user, so the harness must permit
    // authorization below the root invocation.
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);

    let delivery_contract_id = env.register(delivery_contract::DeliveryContract, ());
    let escrow_contract_id = env.register(EscrowContract, ());
    let identity_contract_id =
        env.register(identity_reputation_contract::IdentityReputationContract, ());

    let delivery_client =
        delivery_contract::DeliveryContractClient::new(&env, &delivery_contract_id);
    let escrow_client = EscrowContractClient::new(&env, &escrow_contract_id);
    let identity_client = identity_reputation_contract::IdentityReputationContractClient::new(
        &env,
        &identity_contract_id,
    );

    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    escrow_client.init(&admin, &token, &0);
    delivery_client.init(&admin, &escrow_contract_id);
    // Only delivery_contract needs authority to call increase_reputation here;
    // the dispute_resolution_contract slot is unused by this flow.
    identity_client.init(&admin, &delivery_contract_id, &Address::generate(&env));
    delivery_client.set_identity_reputation_contract(&admin, &identity_contract_id);

    identity_client.register_driver(&driver);
    mint(&env, &token, &sender, amount);

    let metadata = shared_types::DeliveryMetadata {
        delivery_id: 0,
        origin: soroban_sdk::String::from_str(&env, "Origin"),
        destination: soroban_sdk::String::from_str(&env, "Destination"),
        cargo_description: shared_types::CargoDescriptor {
            weight_grams: 500,
            category: shared_types::CargoCategory::Electronics,
            fragile: false,
        },
        created_at: env.ledger().timestamp(),
        estimated_delivery: env.ledger().timestamp() + 3600,
    };

    let delivery_id = delivery_client.create_delivery(&sender, &recipient, &metadata);
    escrow_client.create_escrow(
        &sender,
        &recipient,
        &driver,
        &u64::from(delivery_id),
        &token,
        &amount,
        &None,
    );

    delivery_client.assign_driver(&admin, &delivery_id, &driver);
    delivery_client.mark_in_transit(&driver, &delivery_id);
    delivery_client.confirm_delivery(&recipient, &delivery_id);

    (
        env,
        escrow_contract_id,
        identity_contract_id,
        token,
        admin,
        sender,
        driver,
        u64::from(delivery_id),
    )
}

/// Escrow-only equivalent of the above: drives a single escrow into
/// `Holdback` through `mark_holdback_escrow`, which is exactly the call
/// `delivery_contract::confirm_delivery` makes.
#[allow(clippy::type_complexity)]
fn setup_holdback_escrow(
    delivery_id: u64,
    amount: i128,
) -> (Env, Address, Address, Address, Address, Address, Address) {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, amount);
    client.create_escrow(
        &sender,
        &recipient,
        &driver,
        &delivery_id,
        &token,
        &amount,
        &None,
    );
    client.mark_holdback_escrow(&recipient, &delivery_id);

    (env, contract_id, token, admin, sender, recipient, driver)
}

/// The exact reported exploit, end-to-end through the real delivery ->
/// escrow -> identity_reputation chain.
///
/// Before the fix this call succeeded: the sender's balance was fully
/// restored, the driver was never paid, and the reputation credited during
/// `confirm_delivery` stayed credited — a free delivery plus reputation
/// farming. `refund_escrow` accepted `Holdback` as a refundable state while
/// gating only `Paused` behind the admin check, so the plain sender passed
/// both the authorization and the state check.
#[test]
fn test_sender_cannot_refund_escrow_after_delivery_confirmed() {
    let (env, escrow_id, identity_id, token, admin, sender, driver, delivery_id) =
        setup_confirmed_delivery_in_holdback(1000);
    let escrow_client = EscrowContractClient::new(&env, &escrow_id);
    let identity_client =
        identity_reputation_contract::IdentityReputationContractClient::new(&env, &identity_id);

    // Recipient confirmation put the escrow in Holdback and credited the
    // driver's reputation for the completed delivery.
    assert_eq!(
        escrow_client.get_escrow(&delivery_id).status,
        EscrowStatus::Holdback
    );
    let profile_before = identity_client.get_driver_profile(&driver);
    assert!(profile_before.reputation_score > 50);
    assert_eq!(profile_before.deliveries_completed, 1);

    // The attack: the plain sender tries to reclaim the whole escrow.
    let result = escrow_client.try_refund_escrow(&sender, &delivery_id);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::Unauthorized.into()),
        _ => panic!("Expected FaniLabError::Unauthorized"),
    }

    // Nothing moved: the escrow is still Holdback and the funds are still
    // in custody, earmarked for the driver.
    assert_eq!(
        escrow_client.get_escrow(&delivery_id).status,
        EscrowStatus::Holdback
    );
    assert_eq!(balance(&env, &token, &sender), 0);
    assert_eq!(balance(&env, &token, &driver), 0);
    assert_eq!(balance(&env, &token, &escrow_id), 1000);
    assert_eq!(escrow_client.get_total_locked(&token), 1000);

    // The accounting invariant the report flagged now holds: the reputation
    // credited at confirmation is backed by an actual payment, because the
    // escrow can still only settle to the driver.
    escrow_client.release_holdback_escrow(&admin, &delivery_id);
    assert_eq!(
        escrow_client.get_escrow(&delivery_id).status,
        EscrowStatus::Released
    );
    assert_eq!(balance(&env, &token, &driver), 1000);
    assert_eq!(balance(&env, &token, &sender), 0);
    assert_eq!(escrow_client.get_total_locked(&token), 0);
    let profile_after = identity_client.get_driver_profile(&driver);
    assert_eq!(
        profile_after.reputation_score,
        profile_before.reputation_score
    );
}

/// Contract-level counterpart of the exploit test, with no delivery_contract
/// in the loop: `mark_holdback_escrow` is the only way into `Holdback`, and a
/// sender refund from there must be rejected on-chain regardless of which
/// caller drove the transition.
#[test]
fn test_sender_cannot_refund_holdback_escrow() {
    let (env, contract_id, token, _admin, sender, _recipient, _driver) =
        setup_holdback_escrow(920, 500);
    let client = EscrowContractClient::new(&env, &contract_id);

    let result = client.try_refund_escrow(&sender, &920u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::Unauthorized.into()),
        _ => panic!("Expected FaniLabError::Unauthorized"),
    }

    assert_eq!(client.get_escrow(&920u64).status, EscrowStatus::Holdback);
    assert_eq!(balance(&env, &token, &sender), 0);
    assert_eq!(balance(&env, &token, &contract_id), 500);
    assert_eq!(client.get_total_locked(&token), 500);
}

/// Authorization boundary: `Holdback` is admin-only for refunds, so neither
/// the recipient, the driver, nor an unrelated address may refund either.
/// The recipient in particular must not be able to confirm delivery and then
/// hand the money back to the sender behind the driver's back.
#[test]
fn test_non_admin_callers_cannot_refund_holdback_escrow() {
    let (env, contract_id, token, _admin, _sender, recipient, driver) =
        setup_holdback_escrow(921, 500);
    let client = EscrowContractClient::new(&env, &contract_id);
    let stranger = Address::generate(&env);

    for caller in [recipient, driver, stranger] {
        let result = client.try_refund_escrow(&caller, &921u64);
        match result {
            Err(Ok(err)) => assert_eq!(err, FaniLabError::Unauthorized.into()),
            _ => panic!("Expected FaniLabError::Unauthorized"),
        }
    }

    assert_eq!(client.get_escrow(&921u64).status, EscrowStatus::Holdback);
    assert_eq!(client.get_total_locked(&token), 500);
}

/// The admin arbitration path out of `Holdback` is preserved: the fix closes
/// the unilateral sender refund without disabling refunds. This mirrors the
/// admin gate already applied to `Paused` escrows (Issue #93).
#[test]
fn test_admin_can_still_refund_holdback_escrow() {
    let (env, contract_id, token, admin, sender, _recipient, _driver) =
        setup_holdback_escrow(922, 500);
    let client = EscrowContractClient::new(&env, &contract_id);

    client.refund_escrow(&admin, &922u64);

    assert_eq!(client.get_escrow(&922u64).status, EscrowStatus::Refunded);
    assert_eq!(balance(&env, &token, &sender), 500);
    assert_eq!(balance(&env, &token, &contract_id), 0);
    assert_eq!(client.get_total_locked(&token), 0);
}

/// The normal settlement path out of `Holdback` still pays the driver, with
/// the platform fee split intact.
#[test]
fn test_release_holdback_escrow_still_pays_driver_after_fix() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &500);
    mint(&env, &token, &sender, 1000);
    client.create_escrow(&sender, &recipient, &driver, &923u64, &token, &1000, &None);
    client.mark_holdback_escrow(&recipient, &923u64);

    client.release_holdback_escrow(&recipient, &923u64);

    assert_eq!(client.get_escrow(&923u64).status, EscrowStatus::Released);
    assert_eq!(balance(&env, &token, &driver), 950);
    assert_eq!(balance(&env, &token, &admin), 50);
    assert_eq!(balance(&env, &token, &contract_id), 0);
    assert_eq!(client.get_total_locked(&token), 0);
}

/// The dispute path out of `Holdback` is preserved end-to-end: the
/// dispute_resolution_contract can still freeze a confirmed-but-contested
/// escrow, and the admin can then arbitrate it to a refund. This is the
/// legitimate way a sender gets their money back after delivery confirmation.
#[test]
fn test_holdback_escrow_can_be_frozen_and_refunded_through_dispute() {
    let (env, contract_id, token, admin, sender, _recipient, _driver) =
        setup_holdback_escrow(924, 500);
    let client = EscrowContractClient::new(&env, &contract_id);
    let dispute_contract = Address::generate(&env);
    client.set_dispute_resolution_contract(&admin, &dispute_contract);

    client.freeze_funds(&dispute_contract, &924u64);
    assert_eq!(client.get_escrow(&924u64).status, EscrowStatus::Paused);

    // Even frozen, the sender still cannot self-refund (Issue #93 gate).
    let result = client.try_refund_escrow(&sender, &924u64);
    match result {
        Err(Ok(err)) => assert_eq!(err, FaniLabError::Unauthorized.into()),
        _ => panic!("Expected FaniLabError::Unauthorized"),
    }

    client.resolve_dispute(&admin, &924u64, &false);
    assert_eq!(client.get_escrow(&924u64).status, EscrowStatus::Refunded);
    assert_eq!(balance(&env, &token, &sender), 500);
    assert_eq!(client.get_total_locked(&token), 0);
}

/// Non-regression: the pre-confirmation refund path is untouched. A sender
/// may still reclaim a `Locked` escrow directly, which is what
/// `delivery_contract::cancel_delivery` relies on.
#[test]
fn test_sender_can_still_refund_locked_escrow_after_fix() {
    let (env, contract_id) = setup_env();
    let client = EscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    client.init(&admin, &token, &0);
    mint(&env, &token, &sender, 500);
    client.create_escrow(&sender, &recipient, &driver, &925u64, &token, &500, &None);

    client.refund_escrow(&sender, &925u64);

    assert_eq!(client.get_escrow(&925u64).status, EscrowStatus::Refunded);
    assert_eq!(balance(&env, &token, &sender), 500);
    assert_eq!(client.get_total_locked(&token), 0);
}

/// Non-regression: the sender-initiated cancellation refund still works
/// through the real delivery_contract, which calls `refund_escrow` with the
/// sender as caller while the escrow is still `Locked`.
#[test]
fn test_delivery_cancellation_still_refunds_sender_after_fix() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let driver = Address::generate(&env);

    let delivery_contract_id = env.register(delivery_contract::DeliveryContract, ());
    let escrow_contract_id = env.register(EscrowContract, ());
    let delivery_client =
        delivery_contract::DeliveryContractClient::new(&env, &delivery_contract_id);
    let escrow_client = EscrowContractClient::new(&env, &escrow_contract_id);

    let token_admin = Address::generate(&env);
    let token = setup_token(&env, &token_admin);

    escrow_client.init(&admin, &token, &0);
    delivery_client.init(&admin, &escrow_contract_id);
    mint(&env, &token, &sender, 800);

    let metadata = shared_types::DeliveryMetadata {
        delivery_id: 0,
        origin: soroban_sdk::String::from_str(&env, "Origin"),
        destination: soroban_sdk::String::from_str(&env, "Destination"),
        cargo_description: shared_types::CargoDescriptor {
            weight_grams: 500,
            category: shared_types::CargoCategory::Electronics,
            fragile: false,
        },
        created_at: env.ledger().timestamp(),
        estimated_delivery: env.ledger().timestamp() + 3600,
    };

    let delivery_id = delivery_client.create_delivery(&sender, &recipient, &metadata);
    escrow_client.create_escrow(
        &sender,
        &recipient,
        &driver,
        &u64::from(delivery_id),
        &token,
        &800,
        &None,
    );
    delivery_client.assign_driver(&admin, &delivery_id, &driver);

    delivery_client.cancel_delivery(&sender, &delivery_id);

    assert_eq!(
        escrow_client.get_escrow(&u64::from(delivery_id)).status,
        EscrowStatus::Refunded
    );
    assert_eq!(balance(&env, &token, &sender), 800);
    assert_eq!(escrow_client.get_total_locked(&token), 0);
}

/// Documents the one route from `Holdback` into `Paused`: `raise_dispute`
/// only accepts `Locked`, so a confirmed-delivery escrow can be contested
/// only through `freeze_funds`, which the dispute_resolution_contract calls.
/// Worth pinning because it is what makes the admin refund the sole
/// arbitration exit from `Holdback` when no dispute contract is configured.
#[test]
fn test_raise_dispute_rejected_on_holdback_escrow() {
    let (env, contract_id, _token, _admin, sender, recipient, driver) =
        setup_holdback_escrow(926, 500);
    let client = EscrowContractClient::new(&env, &contract_id);

    for caller in [sender, recipient, driver] {
        let result = client.try_raise_dispute(&caller, &926u64);
        match result {
            Err(Ok(err)) => assert_eq!(err, EscrowError::InvalidState.into()),
            _ => panic!("Expected EscrowError::InvalidState"),
        }
    }

    assert_eq!(client.get_escrow(&926u64).status, EscrowStatus::Holdback);
}

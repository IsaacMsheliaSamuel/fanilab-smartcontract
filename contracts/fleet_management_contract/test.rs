extern crate std;

use super::*;
use escrow_contract::EscrowContract;
use identity_reputation_contract::IdentityReputationContract;
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger as _},
    xdr, Address, Env, Symbol, TryFromVal, TryIntoVal, Val,
};

/// Decode the most recently published event into the (contract, topics, data)
/// shape the SDK <27 `env.events().all()` used to return directly, since
/// SDK 27's `ContractEvents` only exposes the raw XDR form.
fn last_event(env: &Env) -> (Address, soroban_sdk::Vec<Val>, Val) {
    let events = env.events().all();
    let raw = events.events().last().expect("no events emitted").clone();
    let contract_id = raw.contract_id.expect("event missing contract id");
    let address: Address = xdr::ScVal::Address(xdr::ScAddress::Contract(contract_id))
        .try_into_val(env)
        .expect("failed to decode contract address");
    let xdr::ContractEventBody::V0(body) = raw.body;
    let mut topics = soroban_sdk::Vec::new(env);
    for topic in body.topics.iter() {
        topics.push_back(Val::try_from_val(env, topic).expect("failed to decode topic"));
    }
    let data = Val::try_from_val(env, &body.data).expect("failed to decode event data");
    (address, topics, data)
}

fn setup_test() -> (Env, FleetManagementContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(FleetManagementContract, ());
    let client = FleetManagementContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.init(&admin);

    (env, client, admin)
}

/// Helper: register a fleet and return (fleet_id, owner, treasury).
fn register_fleet(
    env: &Env,
    client: &FleetManagementContractClient,
) -> (FleetId, Address, Address) {
    let owner = Address::generate(env);
    let treasury = Address::generate(env);
    let fleet_id = client.register_fleet(&owner, &treasury);
    (fleet_id, owner, treasury)
}

// ── Issue #67 tests ───────────────────────────────────────────────────────────

#[test]
fn test_init_sets_admin_and_counter() {
    let (env, client, admin) = setup_test();

    let stored_admin: Address = env.as_contract(&client.address, || {
        env.storage().instance().get(&StorageKey::Admin).unwrap()
    });
    assert_eq!(stored_admin, admin);

    let counter: u64 = env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .get(&DataKey::FleetCounter)
            .unwrap()
    });
    assert_eq!(counter, 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_init_twice_panics() {
    let (_env, client, admin) = setup_test();
    client.init(&admin);
}

#[test]
fn test_register_fleet_creates_profile_with_expected_fields() {
    let (env, client, _admin) = setup_test();

    let owner = Address::generate(&env);
    let treasury = Address::generate(&env);

    let fleet_id = client.register_fleet(&owner, &treasury);
    assert_eq!(fleet_id, 1);

    let profile = client.get_fleet(&fleet_id);
    assert_eq!(profile.fleet_id, 1);
    assert_eq!(profile.owner, owner);
    assert_eq!(profile.treasury, treasury);
    assert_eq!(profile.total_active_drivers, 0);
}

#[test]
fn test_register_fleet_increments_counter() {
    let (env, client, _admin) = setup_test();

    let owner_a = Address::generate(&env);
    let treasury_a = Address::generate(&env);
    let owner_b = Address::generate(&env);
    let treasury_b = Address::generate(&env);

    let id_a = client.register_fleet(&owner_a, &treasury_a);
    let id_b = client.register_fleet(&owner_b, &treasury_b);

    assert_eq!(id_a, 1);
    assert_eq!(id_b, 2);

    let profile_b = client.get_fleet(&id_b);
    assert_eq!(profile_b.owner, owner_b);
    assert_eq!(profile_b.treasury, treasury_b);
}

#[test]
fn test_register_fleet_emits_event() {
    let (env, client, _admin) = setup_test();

    let owner = Address::generate(&env);
    let treasury = Address::generate(&env);
    let fleet_id = client.register_fleet(&owner, &treasury);

    let last_event = last_event(&env);

    assert_eq!(last_event.0, client.address.clone());

    let topic0: Symbol = Symbol::try_from_val(&env, &last_event.1.get(0).unwrap()).unwrap();
    assert_eq!(topic0, Symbol::new(&env, "fleet_registered"));

    let data: FleetRegisteredEvent =
        FleetRegisteredEvent::try_from_val(&env, &last_event.2).unwrap();
    assert_eq!(data.fleet_id, fleet_id);
    assert_eq!(data.owner, owner);
    assert_eq!(data.treasury, treasury);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_get_fleet_unknown_id_panics() {
    let (_env, client, _admin) = setup_test();
    client.get_fleet(&999);
}

#[test]
fn test_update_fleet_treasury_does_not_apply_immediately() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, old_treasury) = register_fleet(&env, &client);
    let new_treasury = Address::generate(&env);

    client.update_fleet_treasury(&owner, &fleet_id, &new_treasury);

    // Proposing a change must not redirect payouts until confirmed.
    let profile = client.get_fleet(&fleet_id);
    assert_eq!(profile.treasury, old_treasury);

    let pending = client.get_pending_treasury_update(&fleet_id).unwrap();
    assert_eq!(pending.treasury, new_treasury);
}

#[test]
fn test_update_fleet_treasury_emits_proposed_event_immediately() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, treasury) = register_fleet(&env, &client);
    let new_treasury = Address::generate(&env);

    client.update_fleet_treasury(&owner, &fleet_id, &new_treasury);

    let last_event = last_event(&env);

    let topic0: Symbol = Symbol::try_from_val(&env, &last_event.1.get(0).unwrap()).unwrap();
    assert_eq!(topic0, Symbol::new(&env, "fleet_treasury_change_proposed"));

    let data: FleetTreasuryChangeProposedEvent =
        FleetTreasuryChangeProposedEvent::try_from_val(&env, &last_event.2).unwrap();
    assert_eq!(data.fleet_id, fleet_id);
    assert_eq!(data.owner, owner);
    assert_eq!(data.current_treasury, treasury);
    assert_eq!(data.proposed_treasury, new_treasury);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_update_fleet_treasury_rejects_non_owner() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, _owner, _treasury) = register_fleet(&env, &client);
    let attacker = Address::generate(&env);
    let new_treasury = Address::generate(&env);

    client.update_fleet_treasury(&attacker, &fleet_id, &new_treasury);
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_confirm_fleet_treasury_update_before_timelock_panics() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);
    let new_treasury = Address::generate(&env);

    client.update_fleet_treasury(&owner, &fleet_id, &new_treasury);
    // Timelock has not elapsed yet — must panic.
    client.confirm_fleet_treasury_update(&fleet_id);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_confirm_fleet_treasury_update_without_pending_change_panics() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, _owner, _treasury) = register_fleet(&env, &client);

    // No treasury change was ever proposed — must panic.
    client.confirm_fleet_treasury_update(&fleet_id);
}

#[test]
fn test_confirm_fleet_treasury_update_applies_after_timelock() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _old_treasury) = register_fleet(&env, &client);
    let new_treasury = Address::generate(&env);

    client.update_fleet_treasury(&owner, &fleet_id, &new_treasury);
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + TREASURY_CHANGE_TIMELOCK_SECONDS);
    client.confirm_fleet_treasury_update(&fleet_id);

    // Capture the event right after the mutating call — subsequent read-only
    // calls don't emit anything and the test harness only surfaces events
    // from the most recent invocation.
    let last_event = last_event(&env);
    let topic0: Symbol = Symbol::try_from_val(&env, &last_event.1.get(0).unwrap()).unwrap();
    assert_eq!(topic0, Symbol::new(&env, "fleet_treasury_updated"));

    let profile = client.get_fleet(&fleet_id);
    assert_eq!(profile.treasury, new_treasury);
    assert_eq!(client.get_pending_treasury_update(&fleet_id), None);
}

// ── Issue #68 tests — add_driver_to_fleet ────────────────────────────────────

#[test]
fn test_add_driver_stores_pending_invite() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);

    let status = client.get_driver_fleet_status(&fleet_id, &driver);
    assert_eq!(status, Some(DriverFleetStatus::Pending));
}

#[test]
fn test_add_driver_emits_driver_invited_event() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);

    let last_event = last_event(&env);

    let topic0: Symbol = Symbol::try_from_val(&env, &last_event.1.get(0).unwrap()).unwrap();
    assert_eq!(topic0, Symbol::new(&env, "driver_invited"));

    let data: DriverInvitedEvent = DriverInvitedEvent::try_from_val(&env, &last_event.2).unwrap();
    assert_eq!(data.fleet_id, fleet_id);
    assert_eq!(data.driver, driver);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_add_driver_twice_panics() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    // Second invite to the same driver must panic.
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_add_driver_to_unknown_fleet_panics() {
    let (env, client, _admin) = setup_test();
    let caller = Address::generate(&env);
    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&caller, &999, &driver);
}

// ── Issue #109 tests — cancel_invite ──────────────────────────────────────────

#[test]
fn test_cancel_invite_allows_immediate_reinvite() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    client.cancel_invite(&owner, &fleet_id, &driver);

    assert_eq!(client.get_driver_fleet_status(&fleet_id, &driver), None);

    // Re-inviting immediately afterward must succeed (no DriverAlreadyInvited panic).
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    let status = client.get_driver_fleet_status(&fleet_id, &driver);
    assert_eq!(status, Some(DriverFleetStatus::Pending));
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_cancel_invite_non_signer_is_rejected() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);

    let attacker = Address::generate(&env);
    client.cancel_invite(&attacker, &fleet_id, &driver);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_cancel_invite_with_no_invite_panics() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.cancel_invite(&owner, &fleet_id, &driver);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_cancel_invite_on_active_driver_panics() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    client.accept_fleet_invite(&fleet_id, &driver);

    client.cancel_invite(&owner, &fleet_id, &driver);
}

// Issue #74 — Fleet Owner Authorization ─────────────────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_add_driver_non_owner_is_rejected() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, _owner, _treasury) = register_fleet(&env, &client);

    let attacker = Address::generate(&env);
    let driver = Address::generate(&env);
    // attacker is not the fleet owner — must panic with Unauthorized.
    client.add_driver_to_fleet(&attacker, &fleet_id, &driver);
}

#[test]
fn test_add_driver_only_owner_can_invite() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    // Fleet owner successfully invites a driver.
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    assert_eq!(
        client.get_driver_fleet_status(&fleet_id, &driver),
        Some(DriverFleetStatus::Pending)
    );
}

// ── Issue #69 tests — accept_fleet_invite ────────────────────────────────────

#[test]
fn test_accept_invite_promotes_driver_to_active() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    client.accept_fleet_invite(&fleet_id, &driver);

    let status = client.get_driver_fleet_status(&fleet_id, &driver);
    assert_eq!(status, Some(DriverFleetStatus::Active));
}

#[test]
fn test_accept_invite_increments_active_driver_count() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver_a = Address::generate(&env);
    let driver_b = Address::generate(&env);

    client.add_driver_to_fleet(&owner, &fleet_id, &driver_a);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver_b);

    client.accept_fleet_invite(&fleet_id, &driver_a);
    let profile = client.get_fleet(&fleet_id);
    assert_eq!(profile.total_active_drivers, 1);

    client.accept_fleet_invite(&fleet_id, &driver_b);
    let profile = client.get_fleet(&fleet_id);
    assert_eq!(profile.total_active_drivers, 2);
}

#[test]
fn test_accept_invite_emits_event() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    client.accept_fleet_invite(&fleet_id, &driver);

    let last_event = last_event(&env);

    let topic0: Symbol = Symbol::try_from_val(&env, &last_event.1.get(0).unwrap()).unwrap();
    assert_eq!(topic0, Symbol::new(&env, "invite_accepted"));
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_accept_invite_without_prior_invite_panics() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, _owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    // No invite was sent — must panic.
    client.accept_fleet_invite(&fleet_id, &driver);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_accept_invite_twice_panics() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    client.accept_fleet_invite(&fleet_id, &driver);
    // Accepting again must panic.
    client.accept_fleet_invite(&fleet_id, &driver);
}

// ── Issue #70 tests — remove_driver_from_fleet ───────────────────────────────

#[test]
fn test_remove_active_driver_decrements_count() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    client.accept_fleet_invite(&fleet_id, &driver);

    // Owner removes the driver.
    client.remove_driver_from_fleet(&fleet_id, &owner, &driver);

    let profile = client.get_fleet(&fleet_id);
    assert_eq!(profile.total_active_drivers, 0);

    let status = client.get_driver_fleet_status(&fleet_id, &driver);
    assert_eq!(status, Some(DriverFleetStatus::Removed));
}

#[test]
fn test_remove_pending_driver_does_not_affect_active_count() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    // Driver has NOT accepted — still Pending.

    client.remove_driver_from_fleet(&fleet_id, &owner, &driver);

    let profile = client.get_fleet(&fleet_id);
    assert_eq!(profile.total_active_drivers, 0);

    let status = client.get_driver_fleet_status(&fleet_id, &driver);
    assert_eq!(status, Some(DriverFleetStatus::Removed));
}

#[test]
fn test_driver_can_remove_themselves() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    client.accept_fleet_invite(&fleet_id, &driver);

    // Driver removes themselves (caller == driver).
    client.remove_driver_from_fleet(&fleet_id, &driver, &driver);

    let status = client.get_driver_fleet_status(&fleet_id, &driver);
    assert_eq!(status, Some(DriverFleetStatus::Removed));
}

#[test]
fn test_remove_driver_emits_event() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    client.remove_driver_from_fleet(&fleet_id, &owner, &driver);

    let last_event = last_event(&env);

    let topic0: Symbol = Symbol::try_from_val(&env, &last_event.1.get(0).unwrap()).unwrap();
    assert_eq!(topic0, Symbol::new(&env, "driver_removed"));
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_remove_driver_unknown_fleet_panics() {
    let (env, client, _admin) = setup_test();
    let caller = Address::generate(&env);
    let driver = Address::generate(&env);
    client.remove_driver_from_fleet(&999, &caller, &driver);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_remove_driver_not_in_fleet_panics() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    // Driver was never invited — must panic.
    client.remove_driver_from_fleet(&fleet_id, &owner, &driver);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_remove_driver_unauthorized_caller_panics() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);

    let random_caller = Address::generate(&env);
    // random_caller is neither owner nor driver — must panic.
    client.remove_driver_from_fleet(&fleet_id, &random_caller, &driver);
}

// ── Issue #75 tests — Fleet Roster Management ────────────────────────────────

#[test]
fn test_roster_full_lifecycle_add_accept_remove() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);

    // Add: driver starts as Pending.
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    assert_eq!(
        client.get_driver_fleet_status(&fleet_id, &driver),
        Some(DriverFleetStatus::Pending)
    );

    // Accept: driver transitions to Active, count increments.
    client.accept_fleet_invite(&fleet_id, &driver);
    assert_eq!(
        client.get_driver_fleet_status(&fleet_id, &driver),
        Some(DriverFleetStatus::Active)
    );
    assert_eq!(client.get_fleet(&fleet_id).total_active_drivers, 1);

    // Remove: record deleted, count decrements.
    client.remove_driver_from_fleet(&fleet_id, &owner, &driver);
    assert_eq!(
        client.get_driver_fleet_status(&fleet_id, &driver),
        Some(DriverFleetStatus::Removed)
    );
    assert_eq!(client.get_fleet(&fleet_id).total_active_drivers, 0);
}

#[test]
fn test_roster_multiple_drivers_independent_states() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver_a = Address::generate(&env);
    let driver_b = Address::generate(&env);
    let driver_c = Address::generate(&env);

    client.add_driver_to_fleet(&owner, &fleet_id, &driver_a);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver_b);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver_c);

    // Accept only a and b.
    client.accept_fleet_invite(&fleet_id, &driver_a);
    client.accept_fleet_invite(&fleet_id, &driver_b);

    assert_eq!(client.get_fleet(&fleet_id).total_active_drivers, 2);
    assert_eq!(
        client.get_driver_fleet_status(&fleet_id, &driver_c),
        Some(DriverFleetStatus::Pending)
    );

    // Remove driver_a; driver_b and driver_c unaffected.
    client.remove_driver_from_fleet(&fleet_id, &owner, &driver_a);
    assert_eq!(client.get_fleet(&fleet_id).total_active_drivers, 1);
    assert_eq!(
        client.get_driver_fleet_status(&fleet_id, &driver_b),
        Some(DriverFleetStatus::Active)
    );
    assert_eq!(
        client.get_driver_fleet_status(&fleet_id, &driver_c),
        Some(DriverFleetStatus::Pending)
    );
}

#[test]
fn test_roster_driver_can_leave_voluntarily() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    client.accept_fleet_invite(&fleet_id, &driver);

    // Driver removes themselves.
    client.remove_driver_from_fleet(&fleet_id, &driver, &driver);

    assert_eq!(
        client.get_driver_fleet_status(&fleet_id, &driver),
        Some(DriverFleetStatus::Removed)
    );
    assert_eq!(client.get_fleet(&fleet_id).total_active_drivers, 0);
}

#[test]
fn test_roster_re_invite_after_removal() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    client.accept_fleet_invite(&fleet_id, &driver);
    client.remove_driver_from_fleet(&fleet_id, &owner, &driver);

    // Should be possible to invite the same driver again after removal.
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    assert_eq!(
        client.get_driver_fleet_status(&fleet_id, &driver),
        Some(DriverFleetStatus::Pending)
    );
}

// ── Issue #76 tests — Treasury Routing Logic ─────────────────────────────────

#[test]
fn test_get_payout_address_returns_treasury_for_active_driver() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    client.accept_fleet_invite(&fleet_id, &driver);

    let payout = client.get_payout_address(&driver, &fleet_id);
    assert_eq!(payout, treasury);
}

#[test]
fn test_get_payout_address_returns_driver_when_not_in_fleet() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, _owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    // Driver has no record in the fleet.
    let payout = client.get_payout_address(&driver, &fleet_id);
    assert_eq!(payout, driver);
}

#[test]
fn test_get_payout_address_returns_driver_for_pending_invite() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    // Invite is Pending — not yet accepted.

    let payout = client.get_payout_address(&driver, &fleet_id);
    assert_eq!(payout, driver);
}

#[test]
fn test_get_payout_address_returns_driver_after_removal() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    client.accept_fleet_invite(&fleet_id, &driver);
    client.remove_driver_from_fleet(&fleet_id, &owner, &driver);

    // After removal the driver should receive their own address.
    let payout = client.get_payout_address(&driver, &fleet_id);
    assert_eq!(payout, driver);
}

// ── Issue #110 tests — set_identity_contract coverage ─────────────────────────

#[test]
fn test_set_identity_contract_admin_success() {
    let (env, client, admin) = setup_test();

    let identity_id = env.register(IdentityReputationContract, ());
    client.set_identity_contract(&admin, &identity_id);

    let stored: Address = env.as_contract(&client.address, || {
        env.storage()
            .instance()
            .get(&DataKey::IdentityContract)
            .unwrap()
    });
    assert_eq!(stored, identity_id);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_set_identity_contract_unauthorized_caller_panics() {
    let (env, client, _admin) = setup_test();

    let identity_id = env.register(IdentityReputationContract, ());
    let not_admin = Address::generate(&env);
    client.set_identity_contract(&not_admin, &identity_id);
}

// ── Issue #73 tests — register_fleet with identity contract ──────────────────

#[test]
fn test_register_fleet_twice_same_owner_with_identity_contract() {
    let (env, client, admin) = setup_test();

    let identity_id = env.register(IdentityReputationContract, ());
    let identity_client =
        identity_reputation_contract::IdentityReputationContractClient::new(&env, &identity_id);

    client.set_identity_contract(&admin, &identity_id);

    let owner = Address::generate(&env);
    let treasury_a = Address::generate(&env);
    let treasury_b = Address::generate(&env);

    let fleet_id_a = client.register_fleet(&owner, &treasury_a);
    assert_eq!(fleet_id_a, 1);
    assert_eq!(client.get_fleet(&fleet_id_a).owner, owner);

    let fleet_id_b = client.register_fleet(&owner, &treasury_b);
    assert_eq!(fleet_id_b, 2);
    assert_eq!(client.get_fleet(&fleet_id_b).owner, owner);

    assert!(identity_client.has_driver_profile(&owner));
}

#[test]
fn test_register_fleet_for_existing_driver_succeeds() {
    let (env, client, admin) = setup_test();

    let identity_id = env.register(IdentityReputationContract, ());
    let identity_client =
        identity_reputation_contract::IdentityReputationContractClient::new(&env, &identity_id);

    client.set_identity_contract(&admin, &identity_id);

    let owner = Address::generate(&env);
    identity_client.register_driver(&owner);

    let treasury = Address::generate(&env);
    let fleet_id = client.register_fleet(&owner, &treasury);
    assert_eq!(fleet_id, 1);
    assert!(identity_client.has_driver_profile(&owner));
}

#[test]
fn test_get_payout_address_treasury_updates_are_reflected_after_confirmation() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _old_treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    client.accept_fleet_invite(&fleet_id, &driver);

    let new_treasury = Address::generate(&env);
    client.update_fleet_treasury(&owner, &fleet_id, &new_treasury);
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + TREASURY_CHANGE_TIMELOCK_SECONDS);
    client.confirm_fleet_treasury_update(&fleet_id);

    let payout = client.get_payout_address(&driver, &fleet_id);
    assert_eq!(payout, new_treasury);
}

#[test]
fn test_get_payout_address_uses_old_treasury_during_timelock_delay() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, old_treasury) = register_fleet(&env, &client);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    client.accept_fleet_invite(&fleet_id, &driver);

    let new_treasury = Address::generate(&env);
    client.update_fleet_treasury(&owner, &fleet_id, &new_treasury);

    // Still within the timelock delay — payouts must keep routing to the
    // old treasury until the change is confirmed.
    let payout = client.get_payout_address(&driver, &fleet_id);
    assert_eq!(payout, old_treasury);
}

#[test]
fn test_get_payout_address_multiple_drivers_same_fleet() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, treasury) = register_fleet(&env, &client);

    let driver_a = Address::generate(&env);
    let driver_b = Address::generate(&env);
    let driver_c = Address::generate(&env);

    client.add_driver_to_fleet(&owner, &fleet_id, &driver_a);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver_b);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver_c);

    // Only a and b accept; c stays pending.
    client.accept_fleet_invite(&fleet_id, &driver_a);
    client.accept_fleet_invite(&fleet_id, &driver_b);

    assert_eq!(client.get_payout_address(&driver_a, &fleet_id), treasury);
    assert_eq!(client.get_payout_address(&driver_b, &fleet_id), treasury);
    assert_eq!(client.get_payout_address(&driver_c, &fleet_id), driver_c);
}

// ── Issue #71 multi-signature tests ───────────────────────────────────────────

#[test]
fn test_single_owner_fleet_is_backward_compatible() {
    let (env, client, _admin) = setup_test();

    let owner = Address::generate(&env);
    let treasury = Address::generate(&env);
    let fleet_id = client.register_fleet(&owner, &treasury);

    let profile = client.get_fleet(&fleet_id);
    assert_eq!(profile.signers.len(), 1u32);
    assert_eq!(profile.signature_threshold, 1u32);

    let (signers, threshold) = client.get_fleet_signers(&fleet_id);
    assert_eq!(signers.len(), 1u32);
    assert_eq!(threshold, 1u32);
}

#[test]
fn test_configure_signers_adds_multiple_signers() {
    let (env, client, _admin) = setup_test();

    let owner = Address::generate(&env);
    let treasury = Address::generate(&env);
    let fleet_id = client.register_fleet(&owner, &treasury);

    let signer2 = Address::generate(&env);
    let signer3 = Address::generate(&env);

    let mut new_signers = soroban_sdk::Vec::new(&env);
    new_signers.push_back(owner.clone());
    new_signers.push_back(signer2.clone());
    new_signers.push_back(signer3.clone());

    client.configure_signers(&owner, &fleet_id, &new_signers, &2u32);

    let profile = client.get_fleet(&fleet_id);
    assert_eq!(profile.signers.len(), 3u32);
    assert_eq!(profile.signature_threshold, 2u32);
}

#[test]
fn test_configure_signers_unauthorized_not_owner() {
    let (env, client, _admin) = setup_test();

    let owner = Address::generate(&env);
    let treasury = Address::generate(&env);
    let fleet_id = client.register_fleet(&owner, &treasury);

    let attacker = Address::generate(&env);
    let signer2 = Address::generate(&env);

    let mut new_signers = soroban_sdk::Vec::new(&env);
    new_signers.push_back(owner.clone());
    new_signers.push_back(signer2.clone());

    let result = client.try_configure_signers(&attacker, &fleet_id, &new_signers, &2u32);
    match result {
        Err(Ok(err)) => assert_eq!(err, FleetError::Unauthorized.into()),
        _ => panic!("Expected FleetError::Unauthorized"),
    }
}

#[test]
fn test_update_fleet_treasury_with_authorized_signer() {
    let (env, client, _admin) = setup_test();

    let owner = Address::generate(&env);
    let treasury = Address::generate(&env);
    let fleet_id = client.register_fleet(&owner, &treasury);

    let new_treasury = Address::generate(&env);
    client.update_fleet_treasury(&owner, &fleet_id, &new_treasury);

    // update_fleet_treasury only proposes the change (Issue #70 timelock);
    // it must be confirmed after the timelock elapses to actually apply.
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + TREASURY_CHANGE_TIMELOCK_SECONDS);
    client.confirm_fleet_treasury_update(&fleet_id);

    let profile = client.get_fleet(&fleet_id);
    assert_eq!(profile.treasury, new_treasury);
}

#[test]
fn test_update_fleet_treasury_unauthorized_not_signer() {
    let (env, client, _admin) = setup_test();

    let owner = Address::generate(&env);
    let treasury = Address::generate(&env);
    let fleet_id = client.register_fleet(&owner, &treasury);

    let attacker = Address::generate(&env);
    let new_treasury = Address::generate(&env);

    let result = client.try_update_fleet_treasury(&attacker, &fleet_id, &new_treasury);
    match result {
        Err(Ok(err)) => assert_eq!(err, FleetError::Unauthorized.into()),
        _ => panic!("Expected FleetError::Unauthorized"),
    }
}

#[test]
fn test_add_driver_authorized_signer_allowed() {
    let (env, client, _admin) = setup_test();

    let owner = Address::generate(&env);
    let treasury = Address::generate(&env);
    let fleet_id = client.register_fleet(&owner, &treasury);

    let signer2 = Address::generate(&env);
    let mut new_signers = soroban_sdk::Vec::new(&env);
    new_signers.push_back(owner.clone());
    new_signers.push_back(signer2.clone());

    client.configure_signers(&owner, &fleet_id, &new_signers, &1u32);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&signer2, &fleet_id, &driver);

    let status = client.get_driver_fleet_status(&fleet_id, &driver);
    assert_eq!(status, Some(DriverFleetStatus::Pending));
}

#[test]
fn test_add_driver_unauthorized_not_signer() {
    let (env, client, _admin) = setup_test();

    let owner = Address::generate(&env);
    let treasury = Address::generate(&env);
    let fleet_id = client.register_fleet(&owner, &treasury);

    let attacker = Address::generate(&env);
    let driver = Address::generate(&env);

    let result = client.try_add_driver_to_fleet(&attacker, &fleet_id, &driver);
    match result {
        Err(Ok(err)) => assert_eq!(err, FleetError::Unauthorized.into()),
        _ => panic!("Expected FleetError::Unauthorized"),
    }
}

#[test]
fn test_remove_driver_by_authorized_signer() {
    let (env, client, _admin) = setup_test();

    let owner = Address::generate(&env);
    let treasury = Address::generate(&env);
    let fleet_id = client.register_fleet(&owner, &treasury);

    let signer2 = Address::generate(&env);
    let mut new_signers = soroban_sdk::Vec::new(&env);
    new_signers.push_back(owner.clone());
    new_signers.push_back(signer2.clone());

    client.configure_signers(&owner, &fleet_id, &new_signers, &1u32);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    client.accept_fleet_invite(&fleet_id, &driver);

    client.remove_driver_from_fleet(&fleet_id, &signer2, &driver);

    let status = client.get_driver_fleet_status(&fleet_id, &driver);
    assert_eq!(status, Some(DriverFleetStatus::Removed));
}

#[test]
fn test_remove_driver_not_signer_but_is_driver() {
    let (env, client, _admin) = setup_test();

    let owner = Address::generate(&env);
    let treasury = Address::generate(&env);
    let fleet_id = client.register_fleet(&owner, &treasury);

    let driver = Address::generate(&env);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    client.accept_fleet_invite(&fleet_id, &driver);

    client.remove_driver_from_fleet(&fleet_id, &driver, &driver);

    let status = client.get_driver_fleet_status(&fleet_id, &driver);
    assert_eq!(status, Some(DriverFleetStatus::Removed));
}

// ── Cross-contract integration tests ──────────────────────────────────────────

#[test]
#[ignore]
fn test_escrow_payout_routes_through_fleet_treasury() {
    let env = Env::default();
    env.mock_all_auths();

    // Set up fleet management contract
    let fleet_contract_id = env.register(FleetManagementContract, ());
    let fleet_client = FleetManagementContractClient::new(&env, &fleet_contract_id);
    let fleet_admin = Address::generate(&env);
    fleet_client.init(&fleet_admin);

    // Set up escrow contract
    let escrow_contract_id = env.register(EscrowContract, ());
    let escrow_client = escrow_contract::EscrowContractClient::new(&env, &escrow_contract_id);
    let escrow_admin = Address::generate(&env);

    // Create a mock token contract address (we'll use a generated address as a placeholder)
    let token = Address::generate(&env);

    // Initialize escrow contract
    escrow_client.init(&escrow_admin, &token, &500); // 5% platform fee

    // Register a fleet with owner and treasury
    let fleet_owner = Address::generate(&env);
    let fleet_treasury = Address::generate(&env);
    let fleet_id = fleet_client.register_fleet(&fleet_owner, &fleet_treasury);
    assert_eq!(fleet_id, 1);

    // Add a driver to the fleet
    let driver = Address::generate(&env);
    fleet_client.add_driver_to_fleet(&fleet_owner, &fleet_id, &driver);

    // Driver accepts the invite
    fleet_client.accept_fleet_invite(&fleet_id, &driver);

    // Verify driver is now active
    let status = fleet_client.get_driver_fleet_status(&fleet_id, &driver);
    assert_eq!(status, Some(DriverFleetStatus::Active));

    // Verify get_payout_address returns the fleet treasury for active drivers
    let payout_address = fleet_client.get_payout_address(&driver, &fleet_id);
    assert_eq!(payout_address, fleet_treasury);

    // When escrow_contract calls get_payout_address with this driver and fleet_id,
    // it should receive the treasury address for routing payouts.
    // This test verifies the integration point is correctly wired.
    // The actual payout routing through this address is tested in GitHub #12.
}

// ── Issue #108 tests — deactivate_fleet ───────────────────────────────────────

#[test]
fn test_owner_can_deactivate_fleet() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    client.deactivate_fleet(&owner, &fleet_id);

    let profile = client.get_fleet(&fleet_id);
    assert!(!profile.active);
}

#[test]
fn test_admin_can_deactivate_fleet() {
    let (env, client, admin) = setup_test();
    let (fleet_id, _owner, _treasury) = register_fleet(&env, &client);

    client.deactivate_fleet(&admin, &fleet_id);

    let profile = client.get_fleet(&fleet_id);
    assert!(!profile.active);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_deactivate_fleet_rejects_unauthorized_caller() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, _owner, _treasury) = register_fleet(&env, &client);
    let stranger = Address::generate(&env);

    client.deactivate_fleet(&stranger, &fleet_id);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_deactivate_fleet_rejects_unknown_fleet() {
    let (env, client, _admin) = setup_test();
    let owner = Address::generate(&env);

    client.deactivate_fleet(&owner, &999);
}

#[test]
fn test_deactivate_fleet_emits_event() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);

    client.deactivate_fleet(&owner, &fleet_id);

    let last_event = last_event(&env);
    let topic0: Symbol = Symbol::try_from_val(&env, &last_event.1.get(0).unwrap()).unwrap();
    assert_eq!(topic0, Symbol::new(&env, "fleet_deactivated"));
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_add_driver_to_fleet_rejects_invite_on_deactivated_fleet() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, _treasury) = register_fleet(&env, &client);
    let driver = Address::generate(&env);

    client.deactivate_fleet(&owner, &fleet_id);
    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
}

#[test]
fn test_get_payout_address_falls_back_to_driver_after_deactivation() {
    let (env, client, _admin) = setup_test();
    let (fleet_id, owner, treasury) = register_fleet(&env, &client);
    let driver = Address::generate(&env);

    client.add_driver_to_fleet(&owner, &fleet_id, &driver);
    client.accept_fleet_invite(&fleet_id, &driver);

    // Active driver in an active fleet routes to the treasury.
    assert_eq!(client.get_payout_address(&driver, &fleet_id), treasury);

    // Once the fleet is deactivated, payouts fall back to the driver's own address.
    client.deactivate_fleet(&owner, &fleet_id);
    assert_eq!(client.get_payout_address(&driver, &fleet_id), driver);
}

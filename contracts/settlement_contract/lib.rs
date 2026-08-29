#![no_std]

use shared_types::{FaniLabError, StorageKey};

// Settlement contract for cross-border currency swaps
// This contract will handle currency conversions between different assets
// during escrow release for international deliveries.
//
// CRITICAL: This contract is currently a stub. Phase 3 implementation is required
// before mainnet deployment. See PRODUCTION_READINESS.md "Next Steps for Mainnet Launch."

use soroban_sdk::{contract, contracterror, contractimpl, panic_with_error, Address, Env};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum SettlementError {
    SwapNotImplemented = 1,
}

#[contract]
pub struct SettlementContract;

#[contractimpl]
impl SettlementContract {
    /// Initialize the settlement contract
    pub fn init(env: Env, admin: Address) {
        if env.storage().instance().has(&StorageKey::Admin) {
            panic_with_error!(&env, FaniLabError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&StorageKey::Admin, &admin);
        // Implementation to be added in Phase 3
    }

    /// Get the settlement contract administrator.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&StorageKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::NotInitialized))
    }

    /// Get driver's preferred asset for payment
    pub fn get_driver_preference(_env: Env, _driver: Address) -> Option<Address> {
        // Implementation to be added in Phase 3
        None
    }

    /// Execute asset swap and transfer to driver.
    ///
    /// PRODUCTION GUARD: This function will panic if called in production before
    /// Phase 3 implementation is complete. This is intentional to prevent silent
    /// no-op swaps on mainnet with unimplemented settlement logic.
    pub fn execute_settlement_swap(
        _env: Env,
        caller: Address,
        _from_token: Address,
        _to_token: Address,
        _recipient: Address,
        _amount: i128,
        _min_amount_out: i128,
    ) {
        caller.require_auth();

        // CRITICAL GUARD: Prevent execution of stub function on mainnet.
        // Phase 3 settlement logic must be implemented before production use.
        panic_with_error!(&_env, SettlementError::SwapNotImplemented);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn test_init() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let contract_id = env.register(SettlementContract, ());
        let client = SettlementContractClient::new(&env, &contract_id);

        client.init(&admin);
        assert_eq!(client.get_admin(), admin);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #2)")]
    fn test_init_rejects_reinitialization() {
        let env = Env::default();
        env.mock_all_auths();

        let first_admin = Address::generate(&env);
        let second_admin = Address::generate(&env);
        let contract_id = env.register(SettlementContract, ());
        let client = SettlementContractClient::new(&env, &contract_id);

        client.init(&first_admin);
        client.init(&second_admin);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1)")]
    fn test_execute_settlement_swap_panics_when_unimplemented() {
        // This test enforces the production guard: execute_settlement_swap must
        // panic until Phase 3 settlement logic is implemented. This prevents
        // accidental deployment with silent no-op currency swaps.
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let caller = Address::generate(&env);
        let from_token = Address::generate(&env);
        let to_token = Address::generate(&env);
        let recipient = Address::generate(&env);

        let contract_id = env.register(SettlementContract, ());
        let client = SettlementContractClient::new(&env, &contract_id);

        client.init(&admin);

        // This call must return the typed swap-not-implemented error to guard
        // against accidental mainnet deployment with unimplemented settlement logic.
        client.execute_settlement_swap(
            &caller,
            &from_token,
            &to_token,
            &recipient,
            &1000i128,
            &900i128,
        );
    }
}

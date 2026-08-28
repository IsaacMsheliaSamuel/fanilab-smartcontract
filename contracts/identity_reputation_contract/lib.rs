#![no_std]

use shared_types::{
    events, is_admin, ttl, DriverProfile, DriverRegisteredEvent, FaniLabError,
    KycStatusUpdatedEvent, ReputationAwardedEvent, ReputationDecreasedEvent,
    ReputationIncreasedEvent, StorageKey, UserProfile, UserRegisteredEvent,
};
use soroban_sdk::{contract, contractimpl, contracttype, panic_with_error, Address, Env};

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ReputationConfig {
    pub base_points: u32,
    pub heavy_cargo_points: u32,
    pub fragile_points: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    UserProfile(Address),
    DriverProfile(Address),
    AuthorizedContract(Address),
    DeliveryContract,
    DisputeContract,
    ReputationConfig,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum DriverTier {
    Bronze,
    Silver,
    Gold,
}

const MAX_REPUTATION: u32 = 100;
const GOLD_TIER_THRESHOLD: u32 = 75;
// Enterprise eligibility is intentionally tied to reaching the Gold tier.
const ENTERPRISE_THRESHOLD: u32 = GOLD_TIER_THRESHOLD;
const HEAVY_CARGO_GRAMS: u32 = 5000;
const DEFAULT_BASE_POINTS: u32 = 5;
const DEFAULT_HEAVY_CARGO_POINTS: u32 = 3;
const DEFAULT_FRAGILE_POINTS: u32 = 2;

#[contract]
pub struct IdentityReputationContract;

#[contractimpl]
impl IdentityReputationContract {
    pub fn init(env: Env, admin: Address, delivery_contract: Address, dispute_contract: Address) {
        if env.storage().instance().has(&StorageKey::Admin) {
            panic_with_error!(&env, FaniLabError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&StorageKey::Admin, &admin);

        // Register the initial two authorized contracts through the allowlist so
        // they can be revoked or rotated later without a contract migration.
        env.storage()
            .persistent()
            .set(&DataKey::AuthorizedContract(delivery_contract), &true);
        env.storage()
            .persistent()
            .set(&DataKey::AuthorizedContract(dispute_contract), &true);
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&StorageKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::NotInitialized))
    }

    pub fn set_authorized_contract(
        env: Env,
        admin: Address,
        contract_addr: Address,
        authorized: bool,
    ) {
        admin.require_auth();
        if !is_admin(&env, &admin) {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }
        let key = DataKey::AuthorizedContract(contract_addr);
        if authorized {
            env.storage().persistent().set(&key, &true);
        } else {
            env.storage().persistent().remove(&key);
        }
    }

    pub fn set_reputation_config(env: Env, admin: Address, config: ReputationConfig) {
        admin.require_auth();
        if !is_admin(&env, &admin) {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }
        env.storage()
            .instance()
            .set(&DataKey::ReputationConfig, &config);
    }

    pub fn get_reputation_config(env: Env) -> ReputationConfig {
        env.storage()
            .instance()
            .get(&DataKey::ReputationConfig)
            .unwrap_or(ReputationConfig {
                base_points: DEFAULT_BASE_POINTS,
                heavy_cargo_points: DEFAULT_HEAVY_CARGO_POINTS,
                fragile_points: DEFAULT_FRAGILE_POINTS,
            })
    }

    pub fn set_delivery_contract(env: Env, admin: Address, delivery_contract: Address) {
        admin.require_auth();
        if !is_admin(&env, &admin) {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }
        env.storage()
            .instance()
            .set(&DataKey::DeliveryContract, &delivery_contract);
    }

    pub fn set_dispute_contract(env: Env, admin: Address, dispute_contract: Address) {
        admin.require_auth();
        if !is_admin(&env, &admin) {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }
        env.storage()
            .instance()
            .set(&DataKey::DisputeContract, &dispute_contract);
    }

    pub fn get_delivery_contract(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::DeliveryContract)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::NotInitialized))
    }

    pub fn get_dispute_contract(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::DisputeContract)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::NotInitialized))
    }

    pub fn is_authorized_contract(env: Env, contract_addr: Address) -> bool {
        let key = DataKey::AuthorizedContract(contract_addr);
        env.storage().persistent().get(&key).unwrap_or(false)
    }

    pub fn has_driver_profile(env: Env, driver: Address) -> bool {
        let key = DataKey::DriverProfile(driver);
        env.storage()
            .persistent()
            .get::<_, DriverProfile>(&key)
            .is_some()
    }

    #[allow(deprecated)] // events().publish() is deprecated in SDK 27.0.0 but still functional; tracked in SOROBAN_SDK_27_MIGRATION.md#event-system-migration (Issue #114)
    pub fn register_driver(env: Env, driver: Address) {
        driver.require_auth();
        let key = DataKey::DriverProfile(driver.clone());
        if env.storage().persistent().has(&key) {
            panic_with_error!(&env, FaniLabError::AlreadyInitialized);
        }

        let profile = DriverProfile {
            address: driver.clone(),
            deliveries_completed: 0,
            reputation_score: 50,
            registered_at: env.ledger().timestamp(),
            kyc_verified: false,
        };

        env.storage().persistent().set(&key, &profile);
        env.storage().persistent().extend_ttl(
            &key,
            ttl::LEDGER_TTL_THRESHOLD,
            ttl::LEDGER_TTL_EXTEND_TO,
        );

        env.events().publish(
            (events::driver_registered(&env),),
            DriverRegisteredEvent { driver },
        );
    }

    #[allow(deprecated)] // events().publish() is deprecated in SDK 27.0.0 but still functional; tracked in SOROBAN_SDK_27_MIGRATION.md#event-system-migration (Issue #114)
    pub fn register_user(env: Env, user: Address) -> UserProfile {
        user.require_auth();

        let registered_at = env.ledger().timestamp();

        let profile = UserProfile {
            address: user.clone(),
            registered_at,
        };

        let key = DataKey::UserProfile(user.clone());
        if env.storage().persistent().has(&key) {
            panic_with_error!(&env, FaniLabError::AlreadyInitialized);
        }

        env.storage().persistent().set(&key, &profile);
        env.storage().persistent().extend_ttl(
            &key,
            ttl::LEDGER_TTL_THRESHOLD,
            ttl::LEDGER_TTL_EXTEND_TO,
        );

        env.events().publish(
            (events::user_registered(&env),),
            UserRegisteredEvent { user },
        );

        profile
    }

    pub fn get_user_profile(env: Env, user: Address) -> UserProfile {
        let key = DataKey::UserProfile(user);
        let profile: UserProfile = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::ProviderNotFound));
        profile
    }

    pub fn get_driver_profile(env: Env, driver: Address) -> DriverProfile {
        let key = DataKey::DriverProfile(driver);
        let profile: DriverProfile = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::ProviderNotFound));
        profile
    }

    #[allow(deprecated)] // events().publish() is deprecated in SDK 27.0.0 but still functional; tracked in SOROBAN_SDK_27_MIGRATION.md#event-system-migration (Issue #114)
    pub fn update_driver_kyc_status(env: Env, admin: Address, driver: Address, kyc_verified: bool) {
        admin.require_auth();

        if !is_admin(&env, &admin) {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }

        let key = DataKey::DriverProfile(driver.clone());
        let mut profile: DriverProfile = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::ProviderNotFound));

        profile.kyc_verified = kyc_verified;

        env.storage().persistent().set(&key, &profile);
        env.storage().persistent().extend_ttl(
            &key,
            ttl::LEDGER_TTL_THRESHOLD,
            ttl::LEDGER_TTL_EXTEND_TO,
        );

        env.events().publish(
            (events::kyc_status_updated(&env),),
            KycStatusUpdatedEvent {
                driver,
                kyc_verified,
            },
        );
    }

    #[allow(deprecated)] // events().publish() is deprecated in SDK 27.0.0 but still functional; tracked in SOROBAN_SDK_27_MIGRATION.md#event-system-migration (Issue #114)
    pub fn increase_reputation(
        env: Env,
        caller: Address,
        driver: Address,
        delivery_id: u64,
        weight_grams: u32,
        fragile: bool,
    ) {
        if !Self::is_authorized_contract(env.clone(), caller.clone()) {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }
        caller.require_auth();

        let key = DataKey::DriverProfile(driver.clone());
        let mut profile: DriverProfile = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::ProviderNotFound));

        let config = Self::get_reputation_config(env.clone());

        let mut points: u32 = config.base_points;
        if weight_grams > HEAVY_CARGO_GRAMS {
            points += config.heavy_cargo_points;
        }
        if fragile {
            points += config.fragile_points;
        }

        profile.reputation_score = (profile.reputation_score + points).min(MAX_REPUTATION);
        profile.deliveries_completed += 1;

        env.storage().persistent().set(&key, &profile);
        env.storage().persistent().extend_ttl(
            &key,
            ttl::LEDGER_TTL_THRESHOLD,
            ttl::LEDGER_TTL_EXTEND_TO,
        );

        env.events().publish(
            (events::reputation_increased(&env),),
            ReputationIncreasedEvent {
                driver,
                delivery_id,
                points,
            },
        );
    }

    #[allow(deprecated)] // events().publish() is deprecated in SDK 27.0.0 but still functional; tracked in SOROBAN_SDK_27_MIGRATION.md#event-system-migration (Issue #114)
    pub fn decrease_reputation(env: Env, caller: Address, driver: Address, points: u32) {
        if !Self::is_authorized_contract(env.clone(), caller.clone()) {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }
        caller.require_auth();

        let key = DataKey::DriverProfile(driver.clone());
        let mut profile: DriverProfile = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::ProviderNotFound));

        profile.reputation_score = profile.reputation_score.saturating_sub(points);

        env.storage().persistent().set(&key, &profile);
        env.storage().persistent().extend_ttl(
            &key,
            ttl::LEDGER_TTL_THRESHOLD,
            ttl::LEDGER_TTL_EXTEND_TO,
        );

        env.events().publish(
            (events::reputation_decreased(&env),),
            ReputationDecreasedEvent { driver, points },
        );
    }

    /// Apply a flat reputation award to a driver, mirroring `decrease_reputation`.
    ///
    /// Unlike `increase_reputation`, this does **not** derive points from cargo
    /// weight/fragility and does **not** increment `deliveries_completed` — a
    /// dispute ruling in the driver's favour is not a delivery completion, and
    /// counting it as one would double-count if the delivery is later confirmed.
    /// The resulting score is still capped at `MAX_REPUTATION`.
    #[allow(deprecated)] // events().publish() is deprecated in SDK 27.0.0 but still functional; tracked in SOROBAN_SDK_27_MIGRATION.md#event-system-migration (Issue #114)
    pub fn award_reputation(env: Env, caller: Address, driver: Address, points: u32) {
        if !Self::is_authorized_contract(env.clone(), caller.clone()) {
            panic_with_error!(&env, FaniLabError::Unauthorized);
        }
        caller.require_auth();

        let key = DataKey::DriverProfile(driver.clone());
        let mut profile: DriverProfile = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, FaniLabError::ProviderNotFound));

        profile.reputation_score = (profile.reputation_score + points).min(MAX_REPUTATION);

        env.storage().persistent().set(&key, &profile);
        env.storage().persistent().extend_ttl(
            &key,
            ttl::LEDGER_TTL_THRESHOLD,
            ttl::LEDGER_TTL_EXTEND_TO,
        );

        env.events().publish(
            (events::reputation_awarded(&env),),
            ReputationAwardedEvent { driver, points },
        );
    }

    pub fn get_driver_tier(env: Env, driver: Address) -> DriverTier {
        let profile = Self::get_driver_profile(env, driver);
        let score = profile.reputation_score;
        if score >= GOLD_TIER_THRESHOLD {
            DriverTier::Gold
        } else if score >= 50 {
            DriverTier::Silver
        } else {
            DriverTier::Bronze
        }
    }

    pub fn is_eligible_for_enterprise(env: Env, driver: Address) -> bool {
        let profile = Self::get_driver_profile(env, driver);
        profile.reputation_score >= ENTERPRISE_THRESHOLD
    }
}

#[cfg(test)]
mod test;

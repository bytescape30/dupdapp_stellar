#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Events, Env, Symbol};

#[contract]
pub struct MockAmm;

#[contractimpl]
impl MockAmm {
    pub fn get_reserves(env: Env) -> (i128, i128) {
        let res_a = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "res_a"))
            .unwrap_or(0);
        let res_b = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "res_b"))
            .unwrap_or(0);
        (res_a, res_b)
    }

    pub fn set_reserves(env: Env, res_a: i128, res_b: i128) {
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "res_a"), &res_a);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "res_b"), &res_b);
    }
}

#[test]
fn test_deep_pool_routing() {
    let env = Env::default();
    let pool_address = env.register_contract(None, MockAmm);
    let router_id = env.register_contract(None, LiquidityRouter);
    let router_client = LiquidityRouterClient::new(&env, &router_id);

    // Set high reserves: 1000
    let amm_client = MockAmmClient::new(&env, &pool_address);
    amm_client.set_reserves(&1000i128, &1000i128);

    // Swap 50 (5% of 1000) -> Should be deep enough (50 * 10 = 500 < 1000)
    let route = router_client.check_and_route(&pool_address, &50i128);
    assert_eq!(route, Route::SorobanAMM);

    // Check no fallback event emitted
    assert_eq!(env.events().all().len(), 0);
}

#[test]
fn test_shallow_pool_routing() {
    let env = Env::default();
    env.mock_all_auths();

    let pool_address = env.register_contract(None, MockAmm);
    let router_id = env.register_contract(None, LiquidityRouter);
    let router_client = LiquidityRouterClient::new(&env, &router_id);

    // Set low reserves: 100
    let amm_client = MockAmmClient::new(&env, &pool_address);
    amm_client.set_reserves(&100i128, &100i128);

    // Swap 20 (20% of 100) -> Should trigger fallback (20 * 10 = 200 >= 100)
    let route = router_client.check_and_route(&pool_address, &20i128);
    assert_eq!(route, Route::StellarClassicDEX);

    // Verify event
    let events = env.events().all();
    assert!(events.len() >= 1);

    let event = events.last().unwrap();
    assert_eq!(event.0, router_id);
    assert_eq!(event.1.len(), 1);
}

#[test]
fn test_borderline_depth() {
    let env = Env::default();
    let pool_address = env.register_contract(None, MockAmm);
    let router_id = env.register_contract(None, LiquidityRouter);
    let router_client = LiquidityRouterClient::new(&env, &router_id);

    // Set reserves: 100
    let amm_client = MockAmmClient::new(&env, &pool_address);
    amm_client.set_reserves(&100i128, &100i128);

    // Swap 10 (10% of 100) -> S * 10 < R ? 10 * 10 < 100 is False (100 < 100 is False)
    // So it should trigger fallback
    let route = router_client.check_and_route(&pool_address, &10i128);
    assert_eq!(route, Route::StellarClassicDEX);
}

#[test]
fn test_initialize_sets_admin_and_requires_auth() {
    let env = Env::default();
    env.mock_all_auths();

    let router_id = env.register_contract(None, LiquidityRouter);
    let router_client = LiquidityRouterClient::new(&env, &router_id);

    let admin = Address::generate(&env);
    router_client.initialize(&admin);

    // Admin is recorded and the allowlist starts empty.
    assert_eq!(router_client.get_admin(), admin);
    assert_eq!(router_client.get_approved_pools().len(), 0);
}

#[test]
#[should_panic(expected = "already initialized")]
fn test_initialize_cannot_be_called_twice() {
    let env = Env::default();
    env.mock_all_auths();

    let router_id = env.register_contract(None, LiquidityRouter);
    let router_client = LiquidityRouterClient::new(&env, &router_id);

    let admin = Address::generate(&env);
    router_client.initialize(&admin);

    // A second call (e.g. by an attacker) must be rejected and must not
    // overwrite the admin or wipe the approved-pools allowlist.
    let attacker = Address::generate(&env);
    router_client.initialize(&attacker);
}

#[test]
fn test_admin_and_approved_pools_live_in_instance_storage() {
    let env = Env::default();
    env.mock_all_auths();

    let router_id = env.register_contract(None, LiquidityRouter);
    let router_client = LiquidityRouterClient::new(&env, &router_id);

    let admin = Address::generate(&env);
    router_client.initialize(&admin);

    // Admin and the approved-pools allowlist are config singletons and must be
    // kept in instance() storage so their TTL is bumped with every contract
    // call, rather than in persistent() storage where they could be archived.
    let instance = env.as_contract(&router_id, || env.storage().instance());
    assert!(instance.has(&DataKey::Admin));
    assert!(instance.has(&DataKey::ApprovedPools));

    let persistent = env.as_contract(&router_id, || env.storage().persistent());
    assert!(!persistent.has(&DataKey::Admin));
    assert!(!persistent.has(&DataKey::ApprovedPools));
}

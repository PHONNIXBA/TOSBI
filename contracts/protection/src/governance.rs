use crate::{
    config, set_persistent, Config, Error, Key, Params, ProtectionContract, ProtectionContractArgs,
    ProtectionContractClient,
};
use soroban_sdk::{contractevent, contractimpl, contracttype, Address, BytesN, Env, Symbol};

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Params(Params),
    Pricer(BytesN<32>),
    Oracle(Address),
    Treasury(Address),
    Asset(Symbol, bool),
    Perpdex(Address, bool),
    Upgrade(BytesN<32>),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PendingChange {
    pub change: Change,
    pub eta: u64,
}

#[contractevent]
pub struct ChangeProposed {
    #[topic]
    pub id: u64,
    pub eta: u64,
}

#[contractevent]
pub struct ChangeExecuted {
    #[topic]
    pub id: u64,
}

#[contractevent]
pub struct ChangeCancelled {
    #[topic]
    pub id: u64,
}

#[contractevent]
pub struct PauseSet {
    paused: bool,
}

#[contractevent]
pub struct VolHashSet {
    hash: BytesN<32>,
}

#[contractimpl]
impl ProtectionContract {
    // admin; eta = now + params.timelock
    pub fn propose(env: Env, change: Change) -> u64 {
        let cfg: Config = config(&env);
        cfg.admin.require_auth();

        let id = env.storage().instance().get(&Key::NextChange).unwrap_or(0);
        env.storage().instance().set(&Key::NextChange, &(id + 1));

        let now = env.ledger().timestamp();
        let eta = now + cfg.params.timelock;
        let pending = PendingChange { change, eta };
        set_persistent(&env, &Key::Change(id), &pending);

        ChangeProposed { id, eta }.publish(&env);

        id
    }

    // admin; NoChange / Timelocked
    pub fn execute(env: Env, id: u64) -> Result<(), Error> {
        let mut cfg = config(&env);
        cfg.admin.require_auth();

        let pending: PendingChange = env
            .storage()
            .persistent()
            .get(&Key::Change(id))
            .ok_or(Error::NoChange)?;

        let now = env.ledger().timestamp();
        if now < pending.eta {
            return Err(Error::Timelocked);
        }

        env.storage().persistent().remove(&Key::Change(id));

        match pending.change {
            Change::Params(params) => cfg.params = params,
            Change::Pricer(pricer) => cfg.pricer = pricer,
            Change::Oracle(address) => cfg.oracle = address,
            Change::Treasury(address) => cfg.treasury = address,
            Change::Asset(asset, enabled) => {
                env.storage().instance().set(&Key::Asset(asset), &enabled)
            }
            Change::Perpdex(address, enabled) => env
                .storage()
                .instance()
                .set(&Key::Perpdex(address), &enabled),
            Change::Upgrade(hash) => env
                .deployer()
                .update_current_contract(soroban_sdk::ContractExecutable::Wasm(hash)),
        }

        env.storage().instance().set(&Key::Config, &cfg);

        ChangeExecuted { id }.publish(&env);

        Ok(())
    }

    // admin; NoChange
    pub fn cancel(env: Env, id: u64) -> Result<(), Error> {
        let cfg = config(&env);
        cfg.admin.require_auth();

        if !env.storage().persistent().has(&Key::Change(id)) {
            return Err(Error::NoChange);
        }

        env.storage().persistent().remove(&Key::Change(id));

        ChangeCancelled { id }.publish(&env);

        Ok(())
    }

    // caller must be owner or operator, else Unauthorized
    pub fn pause(env: Env, caller: Address) -> Result<(), Error> {
        caller.require_auth();

        let cfg = config(&env);
        if caller != cfg.owner && caller != cfg.operator {
            return Err(Error::Unauthorized);
        }

        let paused = env.storage().instance().get(&Key::Paused).unwrap_or(false);
        if !paused {
            env.storage().instance().set(&Key::Paused, &true);
            PauseSet { paused: true }.publish(&env);
        }

        Ok(())
    }

    // admin
    pub fn unpause(env: Env) {
        let cfg = config(&env);
        cfg.admin.require_auth();

        let paused = env.storage().instance().get(&Key::Paused).unwrap_or(false);
        if paused {
            env.storage().instance().set(&Key::Paused, &false);
            PauseSet { paused: false }.publish(&env);
        }
    }

    // operator
    pub fn set_vol_hash(env: Env, hash: BytesN<32>) {
        let cfg = config(&env);
        cfg.operator.require_auth();

        env.storage().instance().set(&Key::VolHash, &hash);

        VolHashSet { hash }.publish(&env);
    }

    pub fn pending_change(env: Env, id: u64) -> Option<PendingChange> {
        env.storage().persistent().get(&Key::Change(id))
    }

    pub fn vol_hash(env: Env) -> Option<BytesN<32>> {
        env.storage().instance().get(&Key::VolHash)
    }
}

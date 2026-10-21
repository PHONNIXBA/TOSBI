#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, token, xdr::ToXdr, Address,
    BytesN, Env, Symbol,
};

const DAY_IN_LEDGERS: u32 = 17_280;
const TTL_THRESHOLD: u32 = 7 * DAY_IN_LEDGERS;
const TTL_EXTEND_TO: u32 = 30 * DAY_IN_LEDGERS;
const BPS: i128 = 10_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    InvalidAmount = 1,
    InvalidQuote = 2,
    QuoteExpired = 3,
    NonceUsed = 4,
    Unauthorized = 5,
    AmountOutOfRange = 6,
    InsufficientCapital = 7,
    WalletCapExceeded = 8,
    NotFound = 9,
    AlreadyReleased = 10,
    WithdrawalPending = 11,
    NoWithdrawal = 12,
    WithdrawalLocked = 13,
    PaymentFailed = 14,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Direction {
    Long = 0,
    Short = 1,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Status {
    Active = 0,
    Released = 1,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Params {
    pub a_min: i128,
    pub a_max: i128,
    pub wallet_cap_bps: u32,
    pub wallet_cap_max: i128,
    pub activation_delay: u64,
    pub withdraw_delay: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
    pub token: Address,
    pub treasury: Address,
    pub pricer: BytesN<32>,
    pub params: Params,
}

/// Signed by the off-chain pricer; the signature covers this struct's canonical XDR.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Quote {
    pub vault: Address,
    pub trader: Address,
    pub asset: Symbol,
    pub direction: Direction,
    pub amount: i128,
    pub qty: i128,
    pub p_start: i128,
    pub p_end: i128,
    pub duration: u64,
    pub max_payout: i128,
    pub premium: i128,
    pub fee: i128,
    pub vol_bps: u32,
    pub quoted_at: u64,
    pub expires_at: u64,
    pub nonce: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Protection {
    pub id: u64,
    pub trader: Address,
    pub asset: Symbol,
    pub direction: Direction,
    pub qty: i128,
    pub p_start: i128,
    pub p_end: i128,
    pub max_payout: i128,
    pub premium: i128,
    pub fee: i128,
    pub t_mint: u64,
    pub t_active: u64,
    pub t_expire: u64,
    pub status: Status,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Withdrawal {
    pub amount: i128,
    pub unlock_at: u64,
}

/// `capital` is pool money (deposits + premiums − withdrawals); `locked` is queued for withdrawal.
/// Invariant: reserved ≤ capital − locked.
#[contracttype]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Vault {
    pub capital: i128,
    pub reserved: i128,
    pub locked: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultState {
    pub capital: i128,
    pub reserved: i128,
    pub locked: i128,
    pub unlock_at: u64,
}

#[contracttype]
enum Key {
    Config,
    Vault,
    NextId,
    Withdrawal,
    Protection(u64),
    WalletReserved(Address),
    Nonce(u64),
}

#[contractevent]
pub struct CapitalDeposited {
    #[topic]
    pub from: Address,
    pub amount: i128,
}

#[contractevent]
pub struct Minted {
    #[topic]
    pub id: u64,
    #[topic]
    pub trader: Address,
    pub max_payout: i128,
    pub premium: i128,
    pub fee: i128,
}

#[contractevent]
pub struct Released {
    #[topic]
    pub id: u64,
    pub max_payout: i128,
}

#[contractevent]
pub struct WithdrawalRequested {
    pub amount: i128,
    pub unlock_at: u64,
}

#[contractevent]
pub struct WithdrawalExecuted {
    pub amount: i128,
}

#[contract]
pub struct ProtectionContract;

#[contractimpl]
impl ProtectionContract {
    pub fn __constructor(
        env: Env,
        admin: Address,
        token: Address,
        treasury: Address,
        pricer: BytesN<32>,
        params: Params,
    ) {
        let s = env.storage().instance();
        s.set(
            &Key::Config,
            &Config {
                admin,
                token,
                treasury,
                pricer,
                params,
            },
        );
        s.set(&Key::Vault, &Vault::default());
        s.set(&Key::NextId, &0u64);
    }

    pub fn deposit_capital(env: Env, from: Address, amount: i128) -> Result<(), Error> {
        from.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let cfg = config(&env);
        let mut v = vault(&env);
        v.capital += amount;
        set_vault(&env, &v);
        pay(
            &env,
            &cfg.token,
            &from,
            &env.current_contract_address(),
            amount,
        )?;
        CapitalDeposited { from, amount }.publish(&env);
        Ok(())
    }

    pub fn buy_protection(env: Env, quote: Quote, sig: BytesN<64>) -> Result<u64, Error> {
        let cfg = config(&env);
        quote.trader.require_auth();
        env.crypto()
            .ed25519_verify(&cfg.pricer, &quote.clone().to_xdr(&env), &sig);

        let now = env.ledger().timestamp();
        if now > quote.expires_at {
            return Err(Error::QuoteExpired);
        }
        if quote.vault != env.current_contract_address()
            || quote.premium <= 0
            || quote.fee < 0
            || quote.max_payout <= 0
            || quote.max_payout > quote.amount
            || quote.duration == 0
        {
            return Err(Error::InvalidQuote);
        }
        let nonce_key = Key::Nonce(quote.nonce);
        if env.storage().persistent().has(&nonce_key) {
            return Err(Error::NonceUsed);
        }
        let p = &cfg.params;
        if quote.amount < p.a_min || quote.amount > p.a_max {
            return Err(Error::AmountOutOfRange);
        }

        let mut v = vault(&env);
        v.capital += quote.premium;
        let available = v.capital - v.locked;
        v.reserved += quote.max_payout;
        if v.reserved > available {
            return Err(Error::InsufficientCapital);
        }
        let wallet = wallet_reserved(&env, &quote.trader) + quote.max_payout;
        let wallet_cap = (available * p.wallet_cap_bps as i128 / BPS).min(p.wallet_cap_max);
        if wallet > wallet_cap {
            return Err(Error::WalletCapExceeded);
        }

        set_vault(&env, &v);
        set_persistent(&env, &nonce_key, &true);
        set_persistent(&env, &Key::WalletReserved(quote.trader.clone()), &wallet);
        let id = next_id(&env);
        set_persistent(
            &env,
            &Key::Protection(id),
            &Protection {
                id,
                trader: quote.trader.clone(),
                asset: quote.asset,
                direction: quote.direction,
                qty: quote.qty,
                p_start: quote.p_start,
                p_end: quote.p_end,
                max_payout: quote.max_payout,
                premium: quote.premium,
                fee: quote.fee,
                t_mint: now,
                t_active: now + p.activation_delay,
                t_expire: now + quote.duration,
                status: Status::Active,
            },
        );

        pay(
            &env,
            &cfg.token,
            &quote.trader,
            &env.current_contract_address(),
            quote.premium,
        )?;
        if quote.fee > 0 {
            pay(&env, &cfg.token, &quote.trader, &cfg.treasury, quote.fee)?;
        }
        Minted {
            id,
            trader: quote.trader,
            max_payout: quote.max_payout,
            premium: quote.premium,
            fee: quote.fee,
        }
        .publish(&env);
        Ok(id)
    }

    /// Frees a live reservation with no payout. Stand-in until settlement (Instaward 2) replaces it.
    pub fn release(env: Env, caller: Address, id: u64) -> Result<(), Error> {
        caller.require_auth();
        let cfg = config(&env);
        let key = Key::Protection(id);
        let mut p: Protection = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::NotFound)?;
        if caller != cfg.admin && caller != p.trader {
            return Err(Error::Unauthorized);
        }
        if p.status == Status::Released {
            return Err(Error::AlreadyReleased);
        }
        p.status = Status::Released;
        set_persistent(&env, &key, &p);

        let mut v = vault(&env);
        v.reserved -= p.max_payout;
        set_vault(&env, &v);
        let wallet_key = Key::WalletReserved(p.trader.clone());
        set_persistent(
            &env,
            &wallet_key,
            &(wallet_reserved(&env, &p.trader) - p.max_payout),
        );
        Released {
            id,
            max_payout: p.max_payout,
        }
        .publish(&env);
        Ok(())
    }

    pub fn withdraw_request(env: Env, amount: i128) -> Result<u64, Error> {
        let cfg = config(&env);
        cfg.admin.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if env.storage().instance().has(&Key::Withdrawal) {
            return Err(Error::WithdrawalPending);
        }
        let mut v = vault(&env);
        if amount > v.capital - v.locked - v.reserved {
            return Err(Error::InsufficientCapital);
        }
        v.locked += amount;
        set_vault(&env, &v);
        let unlock_at = env.ledger().timestamp() + cfg.params.withdraw_delay;
        env.storage()
            .instance()
            .set(&Key::Withdrawal, &Withdrawal { amount, unlock_at });
        WithdrawalRequested { amount, unlock_at }.publish(&env);
        Ok(unlock_at)
    }

    pub fn withdraw_execute(env: Env) -> Result<i128, Error> {
        let cfg = config(&env);
        cfg.admin.require_auth();
        let w: Withdrawal = env
            .storage()
            .instance()
            .get(&Key::Withdrawal)
            .ok_or(Error::NoWithdrawal)?;
        if env.ledger().timestamp() < w.unlock_at {
            return Err(Error::WithdrawalLocked);
        }
        let mut v = vault(&env);
        v.capital -= w.amount;
        v.locked -= w.amount;
        set_vault(&env, &v);
        env.storage().instance().remove(&Key::Withdrawal);
        token::Client::new(&env, &cfg.token).transfer(
            &env.current_contract_address(),
            &cfg.admin,
            &w.amount,
        );
        WithdrawalExecuted { amount: w.amount }.publish(&env);
        Ok(w.amount)
    }

    pub fn state(env: Env) -> VaultState {
        let v = vault(&env);
        VaultState {
            capital: v.capital,
            reserved: v.reserved,
            locked: v.locked,
            unlock_at: env
                .storage()
                .instance()
                .get::<_, Withdrawal>(&Key::Withdrawal)
                .map_or(0, |w| w.unlock_at),
        }
    }

    pub fn config(env: Env) -> Config {
        config(&env)
    }

    pub fn protection(env: Env, id: u64) -> Option<Protection> {
        env.storage().persistent().get(&Key::Protection(id))
    }

    pub fn wallet_reserved(env: Env, trader: Address) -> i128 {
        wallet_reserved(&env, &trader)
    }
}

fn config(env: &Env) -> Config {
    env.storage()
        .instance()
        .extend_ttl(TTL_THRESHOLD, TTL_EXTEND_TO);
    env.storage().instance().get(&Key::Config).unwrap()
}

fn vault(env: &Env) -> Vault {
    env.storage().instance().get(&Key::Vault).unwrap()
}

fn set_vault(env: &Env, v: &Vault) {
    env.storage().instance().set(&Key::Vault, v);
}

fn next_id(env: &Env) -> u64 {
    let id: u64 = env.storage().instance().get(&Key::NextId).unwrap();
    env.storage().instance().set(&Key::NextId, &(id + 1));
    id
}

fn wallet_reserved(env: &Env, trader: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&Key::WalletReserved(trader.clone()))
        .unwrap_or(0)
}

/// Token errors would otherwise surface under the token's own error codes, which clash with ours.
fn pay(
    env: &Env,
    token: &Address,
    from: &Address,
    to: &Address,
    amount: i128,
) -> Result<(), Error> {
    match token::Client::new(env, token).try_transfer(from, to, &amount) {
        Ok(Ok(())) => Ok(()),
        _ => Err(Error::PaymentFailed),
    }
}

fn set_persistent<V: soroban_sdk::IntoVal<Env, soroban_sdk::Val>>(env: &Env, key: &Key, val: &V) {
    env.storage().persistent().set(key, val);
    env.storage()
        .persistent()
        .extend_ttl(key, TTL_THRESHOLD, TTL_EXTEND_TO);
}

#[cfg(test)]
mod test;

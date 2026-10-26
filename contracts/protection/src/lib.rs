#![no_std]

mod position;
pub use position::{Position, PositionStatus};

mod oracle;

mod governance;
pub use governance::{Change, PendingChange};

use soroban_sdk::{
    contract, contractclient, contracterror, contractevent, contractimpl, contracttype, token,
    xdr::ToXdr, Address, BytesN, Env, Symbol, Vec,
};

const DAY_IN_LEDGERS: u32 = 17_280;
const TTL_THRESHOLD: u32 = 7 * DAY_IN_LEDGERS;
const TTL_EXTEND_TO: u32 = 30 * DAY_IN_LEDGERS;
const BPS: i128 = 10_000;
const UNIT: u64 = 10_000_000;
const MIN_WINDOW: u64 = 300;
const MAX_WINDOW: u64 = 1_800;

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
    AlreadySettled = 10,
    WithdrawalPending = 11,
    NoWithdrawal = 12,
    WithdrawalLocked = 13,
    PaymentFailed = 14,
    Paused = 15,
    AssetDisabled = 16,
    PerpdexNotAllowed = 17,
    PositionMismatch = 18,
    MarkMoved = 19,
    PositionTooNew = 20,
    AlreadyCovered = 21,
    UtilizationCap = 22,
    ConcentrationCap = 23,
    NotExpired = 24,
    NotClosed = 25,
    OracleStale = 26,
    Timelocked = 27,
    NoChange = 28,
}

#[contractclient(name = "PerpdexClient")]
pub trait PerpdexInterface {
    fn get_position(env: Env, position_id: BytesN<32>) -> Option<Position>;
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Direction {
    Long = 0,
    Short = 1,
}

impl Direction {
    fn opposite(self) -> Self {
        match self {
            Self::Long => Self::Short,
            Self::Short => Self::Long,
        }
    }
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Status {
    Active = 0,
    Settled = 1,
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
    pub util_cap_bps: u32,     // 9000 = 90% of free capital may be reserved
    pub side_cap_bps: u32,     // 2500 = one (asset, direction)
    pub asset_cap_bps: u32,    // 4000 = one asset, both directions
    pub mark_tol_bps: u32,     // 50 = quote p_start within 0.5% of the position's mark
    pub close_tol_bps: u32,    // 150 (used in Task 3)
    pub min_position_age: u64, // 60 s
    pub record_cap_bps: u32,   // 1500 (Task 2)
    pub stale_max: u64,        // 7200 s (Task 2)
    pub big_payout: i128,      // above this max payout, settle on the full 30-min window
    pub share_pdx_bps: u32,    // fee share paid to the owner
    pub timelock: u64,         // Task 4
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
    pub owner: Address,
    pub operator: Address,
    pub token: Address,
    pub treasury: Address,
    pub pricer: BytesN<32>,
    pub oracle: Address,
    pub params: Params,
}

/// Signed by the off-chain pricer; the signature covers this struct's canonical XDR.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Quote {
    // field order doesn't matter for XDR (keys are sorted), names do
    pub vault: Address,
    pub perpdex: Address,
    pub position_id: BytesN<32>,
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
    pub perpdex: Address,
    pub position_id: BytesN<32>,
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
    pub window: u64,
    pub status: Status,
    pub settle_price: i128,
    pub payout: i128, // 0 until settled
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
    // private
    Config,
    Vault,
    NextId,
    NextChange,
    Withdrawal,
    Paused,
    VolHash,
    Asset(Symbol),
    Perpdex(Address),
    Exposure(Symbol, Direction),
    Protection(u64),
    Covered(Address, BytesN<32>),
    WalletReserved(Address),
    Nonce(u64),
    Change(u64),
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
pub struct WithdrawalRequested {
    pub amount: i128,
    pub unlock_at: u64,
}

#[contractevent]
pub struct WithdrawalExecuted {
    pub amount: i128,
}

#[contractevent]
pub struct Settled {
    #[topic]
    pub id: u64,
    pub price: i128,
    pub payout: i128,
    pub early: bool,
    pub degraded: bool,
}

#[contract]
pub struct ProtectionContract;

#[contractimpl]
impl ProtectionContract {
    pub fn __constructor(env: Env, config: Config, assets: Vec<Symbol>, perpdexes: Vec<Address>) {
        let s = env.storage().instance();
        s.set(&Key::Config, &config);
        s.set(&Key::Vault, &Vault::default());
        s.set(&Key::NextId, &0u64);

        for asset in assets {
            s.set(&Key::Asset(asset), &true);
        }

        for perp in perpdexes {
            s.set(&Key::Perpdex(perp), &true);
        }
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

        check_quote(&env, &quote, &cfg)?;

        let (v, side, wallet) = check_capacity(&env, &quote, &cfg)?;

        let p = &cfg.params;

        check_position(&env, &quote, p)?;

        set_exposure(&env, &quote.asset, &quote.direction, side);

        set_vault(&env, &v);

        let nonce_key = Key::Nonce(quote.nonce);
        set_persistent(&env, &nonce_key, &true);

        set_persistent(&env, &Key::WalletReserved(quote.trader.clone()), &wallet);

        let id = next_id(&env);
        let now = env.ledger().timestamp();
        let window = window_for(quote.duration, quote.max_payout, p);

        set_persistent(
            &env,
            &Key::Covered(quote.perpdex.clone(), quote.position_id.clone()),
            &id,
        );

        set_persistent(
            &env,
            &Key::Protection(id),
            &Protection {
                id,
                trader: quote.trader.clone(),
                perpdex: quote.perpdex,
                position_id: quote.position_id,
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
                window,
                settle_price: 0,
                payout: 0,
            },
        );

        let to_pdx = share(quote.fee, p.share_pdx_bps as i128);
        let to_treasury = quote.fee - to_pdx;

        pay(
            &env,
            &cfg.token,
            &quote.trader,
            &env.current_contract_address(),
            quote.premium,
        )?;
        if to_pdx > 0 {
            pay(&env, &cfg.token, &quote.trader, &cfg.owner, to_pdx)?;
        }
        if to_treasury > 0 {
            pay(&env, &cfg.token, &quote.trader, &cfg.treasury, to_treasury)?;
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

    pub fn withdraw_request(env: Env, amount: i128) -> Result<u64, Error> {
        let cfg = config(&env);
        cfg.owner.require_auth();
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
        cfg.owner.require_auth();
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
            &cfg.owner,
            &w.amount,
        );
        WithdrawalExecuted { amount: w.amount }.publish(&env);
        Ok(w.amount)
    }

    pub fn exposure(env: Env, asset: Symbol, direction: Direction) -> i128 {
        exposure(&env, &asset, &direction)
    }

    pub fn next_id(env: Env) -> u64 {
        next_id(&env)
    }

    pub fn paused(env: Env) -> bool {
        paused(&env)
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

    pub fn settle(env: Env, id: u64) -> Result<i128, Error> {
        let mut protection = active(&env, id)?;
        let now = env.ledger().timestamp();
        if now < protection.t_expire {
            return Err(Error::NotExpired);
        }

        let cfg = config(&env);
        let twap_price = oracle::twap(
            &env,
            &cfg.oracle,
            &cfg.params,
            &protection.asset,
            protection.t_expire,
            protection.window,
        );
        let (price, degraded) = match twap_price {
            Some(price) => (price, false),
            None => {
                if now < protection.t_expire + cfg.params.stale_max {
                    return Err(Error::OracleStale);
                }
                let fallback =
                    oracle::last(&env, &cfg.oracle, &protection.asset).ok_or(Error::OracleStale)?;
                (fallback, true)
            }
        };

        finish(&env, &cfg, &mut protection, price, false, degraded)
    }

    pub fn settle_early(env: Env, id: u64) -> Result<i128, Error> {
        let mut protection = active(&env, id)?;

        let position = position::get(&env, &protection.perpdex, &protection.position_id)
            .ok_or(Error::NotClosed)?;

        let reduced = position.status == PositionStatus::Open && position.size < protection.qty;
        if position.status == PositionStatus::Open && !reduced {
            return Err(Error::NotClosed);
        }

        let now = env.ledger().timestamp();
        let ts = if reduced {
            now
        } else {
            position.close_ts.unwrap_or(now)
        };

        if ts >= protection.t_expire {
            return Self::settle(env, id);
        }

        let cfg = config(&env);
        if ts < protection.t_active {
            let price = protection.p_start;
            return finish(&env, &cfg, &mut protection, price, true, false);
        }

        let reference = oracle::price_at(&env, &cfg.oracle, &protection.asset, ts)
            .or_else(|| oracle::last(&env, &cfg.oracle, &protection.asset))
            .ok_or(Error::OracleStale)?;

        let price = if let Some(close) = position.close_price {
            if !reduced
                && (close - reference).abs() * BPS <= reference * (cfg.params.close_tol_bps as i128)
            {
                close
            } else {
                reference
            }
        } else {
            reference
        };

        finish(&env, &cfg, &mut protection, price, true, false)
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

pub fn window_for(duration: u64, max_payout: i128, p: &Params) -> u64 {
    if max_payout > p.big_payout {
        return MAX_WINDOW;
    }

    (duration * 16_853 / 1_000)
        .isqrt()
        .clamp(MIN_WINDOW, MAX_WINDOW)
}

fn exposure(env: &Env, asset: &Symbol, direction: &Direction) -> i128 {
    env.storage()
        .instance()
        .get(&Key::Exposure(asset.clone(), *direction))
        .unwrap_or(0)
}

pub fn set_exposure(env: &Env, asset: &Symbol, direction: &Direction, amount: i128) {
    env.storage()
        .instance()
        .set(&Key::Exposure(asset.clone(), *direction), &amount)
}

pub fn share(amount: i128, bps: i128) -> i128 {
    amount * bps / BPS
}

fn paused(env: &Env) -> bool {
    env.storage().instance().get(&Key::Paused).unwrap_or(false)
}

fn check_quote(env: &Env, quote: &Quote, cfg: &Config) -> Result<(), Error> {
    if paused(env) {
        return Err(Error::Paused);
    }

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
        || quote.qty <= 0
        || quote.p_start <= 0
    {
        return Err(Error::InvalidQuote);
    }

    let p = &cfg.params;
    if quote.amount < p.a_min || quote.amount > p.a_max {
        return Err(Error::AmountOutOfRange);
    }

    if !env
        .storage()
        .instance()
        .get(&Key::Asset(quote.asset.clone()))
        .unwrap_or(false)
    {
        return Err(Error::AssetDisabled);
    }

    let nonce_key = Key::Nonce(quote.nonce);
    if env.storage().persistent().has(&nonce_key) {
        return Err(Error::NonceUsed);
    }

    let covered = Key::Covered(quote.perpdex.clone(), quote.position_id.clone());
    if env.storage().persistent().has(&covered) {
        return Err(Error::AlreadyCovered);
    }

    Ok(())
}

fn check_capacity(env: &Env, quote: &Quote, cfg: &Config) -> Result<(Vault, i128, i128), Error> {
    let mut v = vault(env);
    v.capital += quote.premium;
    let available = v.capital - v.locked;
    v.reserved += quote.max_payout;
    if v.reserved > available {
        return Err(Error::InsufficientCapital);
    }

    let p = &cfg.params;
    if v.reserved * BPS > available * p.util_cap_bps as i128 {
        return Err(Error::UtilizationCap);
    }

    let exposure: i128 = env
        .storage()
        .instance()
        .get(&Key::Exposure(quote.asset.clone(), quote.direction))
        .unwrap_or(0);
    let side = exposure + quote.max_payout;
    if side * BPS > available * p.side_cap_bps as i128 {
        return Err(Error::ConcentrationCap);
    }

    let opposite_exposure: i128 = env
        .storage()
        .instance()
        .get(&Key::Exposure(
            quote.asset.clone(),
            quote.direction.opposite(),
        ))
        .unwrap_or(0);
    let both = side + opposite_exposure;
    if both * BPS > available * p.asset_cap_bps as i128 {
        return Err(Error::ConcentrationCap);
    }

    let wallet = wallet_reserved(env, &quote.trader) + quote.max_payout;
    let wallet_cap = (available * p.wallet_cap_bps as i128 / BPS).min(p.wallet_cap_max);
    if wallet > wallet_cap {
        return Err(Error::WalletCapExceeded);
    }

    Ok((v, side, wallet))
}

fn check_position(env: &Env, quote: &Quote, params: &Params) -> Result<(), Error> {
    let allowed = env
        .storage()
        .instance()
        .get(&Key::Perpdex(quote.perpdex.clone()))
        .unwrap_or(false);
    if !allowed {
        return Err(Error::PerpdexNotAllowed);
    }

    let position =
        position::get(env, &quote.perpdex, &quote.position_id).ok_or(Error::PositionMismatch)?;

    if position.status != PositionStatus::Open
        || position.owner != quote.trader
        || position.asset != quote.asset
        || position.direction != quote.direction
        || position.size < quote.qty
    {
        return Err(Error::PositionMismatch);
    }

    if (position.mark_price - quote.p_start).abs() * BPS
        > quote.p_start * params.mark_tol_bps as i128
    {
        return Err(Error::MarkMoved);
    }

    let now = env.ledger().timestamp();
    if now < position.open_ts + params.min_position_age {
        return Err(Error::PositionTooNew);
    }

    Ok(())
}

fn active(env: &Env, id: u64) -> Result<Protection, Error> {
    let protection: Protection = env
        .storage()
        .persistent()
        .get(&Key::Protection(id))
        .ok_or(Error::NotFound)?;
    if protection.status == Status::Settled {
        return Err(Error::AlreadySettled);
    }
    Ok(protection)
}

fn finish(
    env: &Env,
    cfg: &Config,
    p: &mut Protection,
    price: i128,
    early: bool,
    degraded: bool,
) -> Result<i128, Error> {
    let gap = match p.direction {
        Direction::Long => p.p_start - price,
        Direction::Short => price - p.p_start,
    };

    let payout = if gap <= 0 {
        0
    } else {
        (p.qty * gap / UNIT as i128).min(p.max_payout)
    };

    let mut v = vault(env);
    v.capital -= payout;
    v.reserved -= p.max_payout;
    set_vault(env, &v);

    let mut exposure = exposure(env, &p.asset, &p.direction);
    exposure -= p.max_payout;
    set_exposure(env, &p.asset, &p.direction, exposure);

    let mut wallet = wallet_reserved(env, &p.trader.clone());
    wallet -= p.max_payout;
    set_persistent(env, &Key::WalletReserved(p.trader.clone()), &wallet);

    env.storage()
        .persistent()
        .remove(&Key::Covered(p.perpdex.clone(), p.position_id.clone()));

    p.status = Status::Settled;
    p.settle_price = price;
    p.payout = payout;
    set_persistent(env, &Key::Protection(p.id), p);

    if payout > 0 {
        pay(
            env,
            &cfg.token,
            &env.current_contract_address(),
            &p.trader.clone(),
            payout,
        )?;
    }

    Settled {
        id: p.id,
        price,
        payout,
        early,
        degraded,
    }
    .publish(env);

    Ok(payout)
}

#[cfg(test)]
mod test;

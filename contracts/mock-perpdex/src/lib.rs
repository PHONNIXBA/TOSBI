//! Reference PerpDEX Position Contract implementing the TOSBI position interface (spec §7.6 + `open_ts`).
//! Testnet and test fixture only: marks come from a SEP-40 oracle, and the owner may close at any price
//! so the demo can show the vault rejecting an outlier close.
#![no_std]

use soroban_sdk::{
    contract, contractclient, contractevent, contractimpl, contracttype, Address, BytesN, Env,
    Symbol,
};

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
pub enum PositionStatus {
    Open = 0,
    Closed = 1,
    Liquidated = 2,
}

/// Prices are USDC per unit with 7 decimals; `size` is asset units with 7 decimals.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Position {
    pub owner: Address,
    pub asset: Symbol,
    pub direction: Direction,
    pub size: i128,
    pub entry_price: i128,
    pub mark_price: i128,
    pub status: PositionStatus,
    pub open_ts: u64,
    pub close_price: Option<i128>,
    pub close_ts: Option<u64>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Asset {
    Stellar(Address),
    Other(Symbol),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PriceData {
    pub price: i128,
    pub timestamp: u64,
}

#[allow(dead_code)]
#[contractclient(name = "OracleClient")]
pub trait Sep40 {
    fn decimals(env: Env) -> u32;
    fn lastprice(env: Env, asset: Asset) -> Option<PriceData>;
}

#[contractevent]
pub struct PositionClosed {
    #[topic]
    pub position_id: BytesN<32>,
    pub close_price: i128,
    pub close_ts: u64,
    pub status: PositionStatus,
}

#[contracttype]
enum Key {
    Oracle,
    Next,
    Pos(BytesN<32>),
}

#[contract]
pub struct MockPerpdex;

#[contractimpl]
impl MockPerpdex {
    pub fn __constructor(env: Env, oracle: Address) {
        env.storage().instance().set(&Key::Oracle, &oracle);
        env.storage().instance().set(&Key::Next, &0u64);
    }

    pub fn open(
        env: Env,
        owner: Address,
        asset: Symbol,
        direction: Direction,
        size: i128,
    ) -> BytesN<32> {
        owner.require_auth();
        assert!(size > 0, "size must be positive");
        let n: u64 = env.storage().instance().get(&Key::Next).unwrap();
        env.storage().instance().set(&Key::Next, &(n + 1));
        let mut raw = [0u8; 32];
        raw[24..].copy_from_slice(&n.to_be_bytes());
        let id = BytesN::from_array(&env, &raw);
        let mark = mark(&env, &asset);
        let pos = Position {
            owner,
            asset,
            direction,
            size,
            entry_price: mark,
            mark_price: mark,
            status: PositionStatus::Open,
            open_ts: env.ledger().timestamp(),
            close_price: None,
            close_ts: None,
        };
        env.storage().persistent().set(&Key::Pos(id.clone()), &pos);
        id
    }

    pub fn get_position(env: Env, position_id: BytesN<32>) -> Position {
        let mut pos = load(&env, &position_id);
        if pos.status == PositionStatus::Open {
            pos.mark_price = mark(&env, &pos.asset);
        }
        pos
    }

    /// Closes at `price`, or at the oracle mark when `price` is None.
    pub fn close(env: Env, position_id: BytesN<32>, price: Option<i128>) {
        end(&env, position_id, price, PositionStatus::Closed);
    }

    pub fn liquidate(env: Env, position_id: BytesN<32>, price: Option<i128>) {
        end(&env, position_id, price, PositionStatus::Liquidated);
    }

    /// Partial close: shrinks the open position to `size`.
    pub fn reduce(env: Env, position_id: BytesN<32>, size: i128) {
        let mut pos = load(&env, &position_id);
        pos.owner.require_auth();
        assert!(
            pos.status == PositionStatus::Open && size > 0 && size < pos.size,
            "bad reduce"
        );
        pos.size = size;
        env.storage().persistent().set(&Key::Pos(position_id), &pos);
    }
}

fn load(env: &Env, id: &BytesN<32>) -> Position {
    env.storage()
        .persistent()
        .get(&Key::Pos(id.clone()))
        .expect("no such position")
}

fn end(env: &Env, id: BytesN<32>, price: Option<i128>, status: PositionStatus) {
    let mut pos = load(env, &id);
    pos.owner.require_auth();
    assert!(pos.status == PositionStatus::Open, "position not open");
    let close_price = price.unwrap_or_else(|| mark(env, &pos.asset));
    let close_ts = env.ledger().timestamp();
    pos.status = status;
    pos.close_price = Some(close_price);
    pos.close_ts = Some(close_ts);
    env.storage().persistent().set(&Key::Pos(id.clone()), &pos);
    PositionClosed {
        position_id: id,
        close_price,
        close_ts,
        status,
    }
    .publish(env);
}

fn mark(env: &Env, asset: &Symbol) -> i128 {
    let oracle: Address = env.storage().instance().get(&Key::Oracle).unwrap();
    let c = OracleClient::new(env, &oracle);
    let decimals = c.decimals();
    let p = c
        .lastprice(&Asset::Other(asset.clone()))
        .expect("no oracle price")
        .price;
    p / 10i128.pow(decimals - 7)
}

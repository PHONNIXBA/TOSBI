//! The PerpDEX Position Contract interface (spec §7.6 plus `open_ts`). Must stay byte-identical
//! to `contracts/mock-perpdex`: same field names, same enum numbers.

use soroban_sdk::{contractclient, contracttype, Address, BytesN, Env, Symbol};

use crate::Direction;

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

#[allow(dead_code)]
#[contractclient(name = "PerpdexClient")]
pub trait Perpdex {
    fn get_position(env: Env, position_id: BytesN<32>) -> Position;
}

/// None when the PerpDEX call fails (unknown id, contract error).
pub fn get(env: &Env, perpdex: &Address, id: &BytesN<32>) -> Option<Position> {
    match PerpdexClient::new(env, perpdex).try_get_position(id) {
        Ok(Ok(p)) => Some(p),
        _ => None,
    }
}

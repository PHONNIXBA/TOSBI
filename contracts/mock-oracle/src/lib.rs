//! SEP-40 oracle stand-in for tests and the local network: 14 decimals, 5-minute records, and
//! `price` returns the record at the 5-minute boundary at or before `timestamp`. Anyone may set prices.
#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Symbol};

const RESOLUTION: u64 = 300;

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

#[contracttype]
enum Key {
    Price(Symbol, u64),
    Last(Symbol),
}

#[contract]
pub struct MockOracle;

#[contractimpl]
impl MockOracle {
    pub fn decimals(_env: Env) -> u32 {
        14
    }

    pub fn resolution(_env: Env) -> u32 {
        RESOLUTION as u32
    }

    /// Sets the record covering `timestamp`; `price` has 14 decimals.
    pub fn set(env: Env, asset: Symbol, timestamp: u64, price: i128) {
        let ts = timestamp / RESOLUTION * RESOLUTION;
        let s = env.storage().persistent();
        s.set(&Key::Price(asset.clone(), ts), &price);
        if ts >= s.get(&Key::Last(asset.clone())).unwrap_or(0) {
            s.set(&Key::Last(asset), &ts);
        }
    }

    pub fn price(env: Env, asset: Asset, timestamp: u64) -> Option<PriceData> {
        let Asset::Other(sym) = asset else {
            return None;
        };
        let ts = timestamp / RESOLUTION * RESOLUTION;
        env.storage()
            .persistent()
            .get(&Key::Price(sym, ts))
            .map(|price| PriceData {
                price,
                timestamp: ts,
            })
    }

    pub fn lastprice(env: Env, asset: Asset) -> Option<PriceData> {
        let Asset::Other(sym) = asset.clone() else {
            return None;
        };
        let ts: u64 = env.storage().persistent().get(&Key::Last(sym))?;
        Self::price(env, asset, ts)
    }
}

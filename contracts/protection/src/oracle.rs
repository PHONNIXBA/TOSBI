use soroban_sdk::{contractclient, contracttype, Address, Env, Symbol};

use crate::{Params, BPS};

// oracle.rs: SEP-40 types (must match Reflector and mock-oracle)
#[contracttype]
pub enum Asset {
    Stellar(Address),
    Other(Symbol),
}

#[contracttype]
pub struct PriceData {
    pub price: i128,
    pub timestamp: u64,
}

#[allow(dead_code)]
#[contractclient(name = "OracleClient")]
pub trait Sep40 {
    fn decimals(env: Env) -> u32;
    fn resolution(env: Env) -> u32;
    fn price(env: Env, asset: Asset, timestamp: u64) -> Option<PriceData>;
    fn lastprice(env: Env, asset: Asset) -> Option<PriceData>;
}

pub fn to_7(price: i128, decimals: u32) -> i128 {
    if decimals > 7 {
        price / 10_i128.pow(decimals - 7)
    } else if decimals < 7 {
        price * 10_i128.pow(7 - decimals)
    } else {
        price
    }
}

pub fn twap(
    env: &Env,
    oracle: &Address,
    p: &Params,
    asset: &Symbol,
    end: u64,
    window: u64,
) -> Option<i128> {
    let client = OracleClient::new(env, oracle);
    let res = client.resolution() as u64;

    let last_ts = (end / res) * res;
    let result = client.price(&Asset::Other(asset.clone()), &last_ts);
    match result {
        Some(data) if data.timestamp == last_ts => {}
        _ => return None,
    };

    let n = window.div_ceil(res).max(1);
    let prev_ts = last_ts - n * res;
    let result = client.price(&Asset::Other(asset.clone()), &prev_ts);
    let prev_price = match result {
        Some(data) if data.timestamp == prev_ts => Some(to_7(data.price, client.decimals())),
        _ => None,
    };

    let mut prev = prev_price;
    let mut sum: i128 = 0;
    let mut count: u64 = 0;

    for k in (0..n).rev() {
        let ts = last_ts - k * res;
        let result = client.price(&Asset::Other(asset.clone()), &ts);
        let price = match result {
            Some(data) if data.timestamp == ts => to_7(data.price, client.decimals()),
            _ => continue,
        };

        if let Some(prev_price) = prev {
            if (price - prev_price).abs() * BPS > prev_price * (p.record_cap_bps as i128) {
                continue;
            }
        }

        sum += price;
        count += 1;
        prev = Some(price);
    }

    if count == 0 {
        return None;
    }
    Some(sum / count as i128)
}

pub fn price_at(env: &Env, oracle: &Address, asset: &Symbol, ts: u64) -> Option<i128> {
    let client = OracleClient::new(env, oracle);
    let result = client.price(&Asset::Other(asset.clone()), &ts);
    let price_at = match result {
        Some(data) => Some(to_7(data.price, client.decimals())),
        _ => None,
    };
    price_at
} // Task 3

pub fn last(env: &Env, oracle: &Address, asset: &Symbol) -> Option<i128> {
    let client = OracleClient::new(env, oracle);
    let result = client.lastprice(&Asset::Other(asset.clone()));
    let last = match result {
        Some(data) => Some(to_7(data.price, client.decimals())),
        _ => None,
    };
    last
}

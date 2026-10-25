extern crate std;

mod binding;
mod caps;
//mod early;
//mod governance;
//mod settle;

use super::*;
use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    vec, Address, BytesN, Env, Event, IntoVal, Map, Symbol, TryFromVal, Val,
};
use std::{cell::Cell, vec::Vec};
use tosbi_mock_oracle::{Asset, MockOracle, MockOracleClient};
use tosbi_mock_perpdex::{MockPerpdex, MockPerpdexClient};

const USDC: i128 = 10_000_000;
const NOW: u64 = 1_000_000;
const DAY: u64 = 86_400;
const WEEK: u64 = 7 * DAY;
/// Scales a 7-decimal price to the oracle's 14 decimals.
const PX: i128 = 10_000_000;
/// $0.20 with 7 decimals.
const XLM: i128 = 2_000_000;

struct T {
    env: Env,
    id: Address,
    token: Address,
    admin: Address,
    owner: Address,
    operator: Address,
    treasury: Address,
    oracle: Address,
    perpdex: Address,
    key: SigningKey,
    nonce: Cell<u64>,
}

impl T {
    fn c(&self) -> ProtectionContractClient<'_> {
        ProtectionContractClient::new(&self.env, &self.id)
    }
    fn token(&self) -> TokenClient<'_> {
        TokenClient::new(&self.env, &self.token)
    }
    fn dex(&self) -> MockPerpdexClient<'_> {
        MockPerpdexClient::new(&self.env, &self.perpdex)
    }
    fn now(&self) -> u64 {
        self.env.ledger().timestamp()
    }
    fn trader(&self, funds: i128) -> Address {
        let a = Address::generate(&self.env);
        if funds > 0 {
            StellarAssetClient::new(&self.env, &self.token).mint(&a, &funds);
        }
        a
    }
    fn deposit(&self, amount: i128) {
        let lp = self.trader(amount);
        self.c().deposit_capital(&lp, &amount);
    }
    /// Sets the oracle record covering `ts` to a 7-decimal price.
    fn feed(&self, asset: &str, ts: u64, price: i128) {
        MockOracleClient::new(&self.env, &self.oracle).set(
            &Symbol::new(&self.env, asset),
            &ts,
            &(price * PX),
        );
    }
    fn mark(&self, asset: &str) -> i128 {
        let a = Asset::Other(Symbol::new(&self.env, asset));
        MockOracleClient::new(&self.env, &self.oracle)
            .lastprice(&a)
            .unwrap()
            .price
            / PX
    }
    fn open(&self, owner: &Address, asset: &str, dir: Direction, size: i128) -> BytesN<32> {
        let d = match dir {
            Direction::Long => tosbi_mock_perpdex::Direction::Long,
            Direction::Short => tosbi_mock_perpdex::Direction::Short,
        };
        self.dex()
            .open(owner, &Symbol::new(&self.env, asset), &d, &size)
    }
    /// XLM long at $0.20 covering down to $0.15, on a fresh position sized to the quote.
    fn quote(&self, trader: &Address, amount: i128, max_payout: i128, premium: i128) -> Quote {
        let pos = self.open(trader, "XLM", Direction::Long, amount * 5);
        self.quote_on(trader, pos, amount, max_payout, premium)
    }
    fn quote_on(
        &self,
        trader: &Address,
        position_id: BytesN<32>,
        amount: i128,
        max_payout: i128,
        premium: i128,
    ) -> Quote {
        let n = self.nonce.get();
        self.nonce.set(n + 1);
        let now = self.now();
        Quote {
            vault: self.id.clone(),
            perpdex: self.perpdex.clone(),
            position_id,
            trader: trader.clone(),
            asset: symbol_short!("XLM"),
            direction: Direction::Long,
            amount,
            qty: amount * 5,
            p_start: XLM,
            p_end: 1_500_000,
            duration: DAY,
            max_payout,
            premium,
            fee: amount * 30 / 10_000,
            vol_bps: 10_000,
            quoted_at: now,
            expires_at: now + 30,
            nonce: n,
        }
    }
    /// 1,000 USDC on a fresh `asset`/`dir` position at the current oracle price, 25% range.
    fn quote_for(&self, trader: &Address, asset: &str, dir: Direction, max_payout: i128) -> Quote {
        let px = self.mark(asset);
        let qty = 1_000 * USDC * i128::from(UNIT) / px;
        let pos = self.open(trader, asset, dir, qty);
        let mut q = self.quote_on(trader, pos, 1_000 * USDC, max_payout, USDC);
        q.asset = Symbol::new(&self.env, asset);
        q.direction = dir;
        q.qty = qty;
        q.p_start = px;
        q.p_end = match dir {
            Direction::Long => px * 3 / 4,
            Direction::Short => px * 5 / 4,
        };
        q
    }
    fn sign_with(&self, key: &SigningKey, q: &Quote) -> BytesN<64> {
        let msg: Vec<u8> = q.clone().to_xdr(&self.env).iter().collect();
        BytesN::from_array(&self.env, &key.sign(&msg).to_bytes())
    }
    fn sign(&self, q: &Quote) -> BytesN<64> {
        self.sign_with(&self.key, q)
    }
    fn buy(&self, q: &Quote) -> Result<u64, Error> {
        match self.c().try_buy_protection(q, &self.sign(q)) {
            Ok(Ok(id)) => Ok(id),
            Err(Ok(e)) => Err(e),
            other => panic!("unexpected result {:?}", other),
        }
    }
    fn assert_invariant(&self) {
        let s = self.c().state();
        assert!(s.reserved >= 0 && s.locked >= 0);
        assert!(
            s.reserved <= s.capital - s.locked,
            "full backing violated: {:?}",
            s
        );
        assert!(self.token().balance(&self.id) >= s.capital);
        let mut exposure = 0;
        for asset in [symbol_short!("XLM"), symbol_short!("BTC")] {
            for dir in [Direction::Long, Direction::Short] {
                exposure += self.c().exposure(&asset, &dir);
            }
        }
        assert_eq!(exposure, s.reserved, "exposure out of sync with reserved");
    }
}

fn params(wallet_cap_bps: u32) -> Params {
    Params {
        a_min: 20 * USDC,
        a_max: 50_000 * USDC,
        wallet_cap_bps,
        wallet_cap_max: 1_000_000 * USDC,
        activation_delay: 300,
        withdraw_delay: WEEK,
        util_cap_bps: 10_000,
        side_cap_bps: 10_000,
        asset_cap_bps: 10_000,
        mark_tol_bps: 50,
        close_tol_bps: 150,
        min_position_age: 0,
        record_cap_bps: 1_500,
        stale_max: 7_200,
        big_payout: 10_000 * USDC,
        share_pdx_bps: 0,
        timelock: DAY,
    }
}

fn setup_with(p: Params) -> T {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(NOW);
    let token = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let key = SigningKey::from_bytes(&[7u8; 32]);
    let oracle = env.register(MockOracle, ());
    let perpdex = env.register(MockPerpdex, (oracle.clone(),));
    let cfg = Config {
        admin: Address::generate(&env),
        owner: Address::generate(&env),
        operator: Address::generate(&env),
        token: token.clone(),
        treasury: Address::generate(&env),
        pricer: BytesN::from_array(&env, &key.verifying_key().to_bytes()),
        oracle: oracle.clone(),
        params: p,
    };
    let assets = vec![&env, symbol_short!("XLM"), symbol_short!("BTC")];
    let id = env.register(
        ProtectionContract,
        (cfg.clone(), assets, vec![&env, perpdex.clone()]),
    );
    let t = T {
        env,
        id,
        token,
        admin: cfg.admin,
        owner: cfg.owner,
        operator: cfg.operator,
        treasury: cfg.treasury,
        oracle,
        perpdex,
        key,
        nonce: Cell::new(1),
    };
    t.feed("XLM", NOW, XLM);
    t.feed("BTC", NOW, 100_000 * USDC);
    t
}

fn setup() -> T {
    setup_with(params(10_000))
}

fn state(capital: i128, reserved: i128, locked: i128, unlock_at: u64) -> VaultState {
    VaultState {
        capital: capital * USDC,
        reserved: reserved * USDC,
        locked: locked * USDC,
        unlock_at,
    }
}

#[test]
fn h1_deposit_buy() {
    let t = setup();
    t.deposit(10_000 * USDC);
    let trader = t.trader(100 * USDC);
    let q = t.quote(&trader, 1_000 * USDC, 250 * USDC, 26 * USDC);

    let id = t.buy(&q).unwrap();
    assert_eq!(t.env.auths()[0].0, trader);
    assert_eq!(t.c().state(), state(10_026, 250, 0, 0));
    assert_eq!(t.token().balance(&t.id), 10_026 * USDC);
    assert_eq!(t.token().balance(&t.treasury), 3 * USDC);
    assert_eq!(t.token().balance(&trader), 71 * USDC);
    assert_eq!(t.c().wallet_reserved(&trader), 250 * USDC);
    let p = t.c().protection(&id).unwrap();
    assert_eq!(
        (p.t_mint, p.t_active, p.t_expire, p.window),
        (NOW, NOW + 300, NOW + DAY, 1_206)
    );
    assert_eq!(
        (p.status, p.max_payout, p.trader.clone()),
        (Status::Active, 250 * USDC, trader.clone())
    );
}

#[test]
fn h4_withdraw_after_delay() {
    let t = setup();
    t.deposit(1_000 * USDC);
    assert_eq!(t.c().withdraw_request(&(400 * USDC)), NOW + WEEK);
    assert_eq!(t.env.auths()[0].0, t.owner);
    assert_eq!(t.c().state(), state(1_000, 0, 400, NOW + WEEK));

    t.env.ledger().set_timestamp(NOW + WEEK);
    assert_eq!(t.c().withdraw_execute(), 400 * USDC);
    assert_eq!(t.c().state(), state(600, 0, 0, 0));
    assert_eq!(t.token().balance(&t.owner), 400 * USDC);
}

#[test]
fn e1_exactly_fill_pool_then_one_unit_more() {
    let t = setup();
    t.deposit(1_000 * USDC);
    let trader = t.trader(10_000 * USDC);
    let q = t.quote(&trader, 3_000 * USDC, 1_001 * USDC + 1, USDC);
    assert_eq!(t.buy(&q), Err(Error::InsufficientCapital));
    let q = t.quote(&trader, 3_000 * USDC, 1_001 * USDC, USDC);
    t.buy(&q).unwrap();
    assert_eq!(t.c().state(), state(1_001, 1_001, 0, 0));
    t.assert_invariant();
}

#[test]
fn e2_amount_bounds() {
    let t = setup();
    t.deposit(100_000 * USDC);
    let trader = t.trader(100_000 * USDC);
    for (amount, ok) in [
        (20 * USDC - 1, false),
        (20 * USDC, true),
        (50_000 * USDC, true),
        (50_000 * USDC + 1, false),
    ] {
        let r = t.buy(&t.quote(&trader, amount, amount / 10, USDC));
        assert_eq!(r.is_ok(), ok, "amount {}", amount);
        if !ok {
            assert_eq!(r, Err(Error::AmountOutOfRange));
        }
    }
}

#[test]
fn e3_wallet_cap_boundary() {
    let t = setup_with(params(200));
    t.deposit(10_000 * USDC - USDC);
    let trader = t.trader(1_000 * USDC);
    // available after premium = 10,000 USDC → 2% = 200 USDC
    let q = t.quote(&trader, 1_000 * USDC, 200 * USDC + 1, USDC);
    assert_eq!(t.buy(&q), Err(Error::WalletCapExceeded));
    let q = t.quote(&trader, 1_000 * USDC, 200 * USDC, USDC);
    t.buy(&q).unwrap();
    assert_eq!(t.c().wallet_reserved(&trader), 200 * USDC);
}

#[test]
fn e4_quote_expiry_boundary() {
    let t = setup();
    t.deposit(1_000 * USDC);
    let trader = t.trader(100 * USDC);
    let q = t.quote(&trader, 100 * USDC, 25 * USDC, USDC);
    t.env.ledger().set_timestamp(q.expires_at);
    t.buy(&q).unwrap();
    let q = t.quote(&trader, 100 * USDC, 25 * USDC, USDC);
    t.env.ledger().set_timestamp(q.expires_at + 1);
    assert_eq!(t.buy(&q), Err(Error::QuoteExpired));
}

#[test]
fn e5_premium_counts_toward_capacity() {
    let t = setup();
    t.deposit(100 * USDC);
    let trader = t.trader(100 * USDC);
    let q = t.quote(&trader, 500 * USDC, 110 * USDC, 10 * USDC);
    t.buy(&q).unwrap();
    assert_eq!(t.c().state(), state(110, 110, 0, 0));
}

#[test]
fn e6_withdraw_unlock_boundary() {
    let t = setup();
    t.deposit(1_000 * USDC);
    t.c().withdraw_request(&(100 * USDC));
    t.env.ledger().set_timestamp(NOW + WEEK - 1);
    assert_eq!(
        t.c().try_withdraw_execute(),
        Err(Ok(Error::WithdrawalLocked))
    );
    t.env.ledger().set_timestamp(NOW + WEEK);
    assert_eq!(t.c().withdraw_execute(), 100 * USDC);
}

#[test]
fn e7_withdraw_request_up_to_free_capital() {
    let t = setup();
    t.deposit(1_000 * USDC);
    let trader = t.trader(100 * USDC);
    t.buy(&t.quote(&trader, 1_000 * USDC, 300 * USDC, 10 * USDC))
        .unwrap();
    // free = 1,010 − 300 = 710
    assert_eq!(
        t.c().try_withdraw_request(&(710 * USDC + 1)),
        Err(Ok(Error::InsufficientCapital))
    );
    t.c().withdraw_request(&(710 * USDC));
    t.assert_invariant();
}

#[test]
fn x1_expired_quote_leaves_no_trace() {
    let t = setup();
    t.deposit(1_000 * USDC);
    let trader = t.trader(100 * USDC);
    let q = t.quote(&trader, 100 * USDC, 25 * USDC, USDC);
    t.env.ledger().set_timestamp(NOW + 31);
    assert_eq!(t.buy(&q), Err(Error::QuoteExpired));
    assert_eq!(t.c().state(), state(1_000, 0, 0, 0));
    assert_eq!(t.token().balance(&trader), 100 * USDC);
}

#[test]
fn x2_x3_s7_signature_must_match_canonical_quote() {
    let t = setup();
    t.deposit(1_000 * USDC);
    let trader = t.trader(100 * USDC);
    let q = t.quote(&trader, 100 * USDC, 25 * USDC, 5 * USDC);
    let sig = t.sign(&q);

    let mut tampered = q.clone();
    tampered.premium = 1;
    assert!(matches!(
        t.c().try_buy_protection(&tampered, &sig),
        Err(Err(_))
    ));

    let other_key = SigningKey::from_bytes(&[9u8; 32]);
    assert!(matches!(
        t.c().try_buy_protection(&q, &t.sign_with(&other_key, &q)),
        Err(Err(_))
    ));

    let mut padded: Map<Symbol, Val> =
        Map::try_from_val(&t.env, &IntoVal::<Env, Val>::into_val(&q, &t.env)).unwrap();
    padded.set(symbol_short!("premium"), 1i128.into_val(&t.env));
    padded.set(Symbol::new(&t.env, "extra"), 42u32.into_val(&t.env));
    let res = t.env.try_invoke_contract::<u64, Error>(
        &t.id,
        &Symbol::new(&t.env, "buy_protection"),
        (padded, sig.clone()).into_val(&t.env),
    );
    assert!(matches!(res, Err(Err(_))));

    assert_eq!(t.c().state(), state(1_000, 0, 0, 0));
    t.c().buy_protection(&q, &sig);
}

#[test]
fn x4_malformed_quote_fields() {
    let t = setup();
    t.deposit(1_000 * USDC);
    let trader = t.trader(100 * USDC);
    let base = t.quote(&trader, 100 * USDC, 25 * USDC, USDC);
    let cases: [fn(&mut Quote); 8] = [
        |q| q.premium = 0,
        |q| q.premium = -1,
        |q| q.fee = -1,
        |q| q.max_payout = 0,
        |q| q.max_payout = q.amount + 1,
        |q| q.duration = 0,
        |q| q.qty = 0,
        |q| q.p_start = 0,
    ];
    for f in cases {
        let mut q = base.clone();
        q.nonce += 1_000;
        f(&mut q);
        assert_eq!(t.buy(&q), Err(Error::InvalidQuote));
    }
    let mut q = base.clone();
    q.vault = Address::generate(&t.env);
    assert_eq!(t.buy(&q), Err(Error::InvalidQuote));
}

#[test]
fn x5_m1_empty_and_exceeded_pool_rejected_without_token_movement() {
    let t = setup();
    assert_eq!(t.c().state(), state(0, 0, 0, 0));
    let trader = t.trader(100 * USDC);
    assert_eq!(
        t.buy(&t.quote(&trader, 100 * USDC, 25 * USDC, USDC)),
        Err(Error::InsufficientCapital)
    );
    t.deposit(10 * USDC);
    assert_eq!(
        t.buy(&t.quote(&trader, 100 * USDC, 25 * USDC, USDC)),
        Err(Error::InsufficientCapital)
    );
    assert_eq!(t.token().balance(&trader), 100 * USDC);
    assert_eq!(t.token().balance(&t.treasury), 0);
    assert_eq!(t.c().state(), state(10, 0, 0, 0));
}

#[test]
fn x6_wallet_cap_across_contracts() {
    let t = setup_with(params(200));
    t.deposit(10_000 * USDC);
    let trader = t.trader(1_000 * USDC);
    t.buy(&t.quote(&trader, 400 * USDC, 100 * USDC, USDC))
        .unwrap();
    t.buy(&t.quote(&trader, 400 * USDC, 100 * USDC, USDC))
        .unwrap();
    assert_eq!(
        t.buy(&t.quote(&trader, 400 * USDC, 100 * USDC, USDC)),
        Err(Error::WalletCapExceeded)
    );
    let other = t.trader(1_000 * USDC);
    t.buy(&t.quote(&other, 400 * USDC, 100 * USDC, USDC))
        .unwrap();
}

#[test]
fn x7_insufficient_trader_balance_reverts_everything() {
    let t = setup();
    t.deposit(1_000 * USDC);
    let trader = t.trader(USDC);
    let q = t.quote(&trader, 100 * USDC, 25 * USDC, 5 * USDC);
    assert_eq!(t.buy(&q), Err(Error::PaymentFailed));
    assert_eq!(t.token().balance(&trader), USDC);
    assert_eq!(t.c().state(), state(1_000, 0, 0, 0));
    assert_eq!(t.c().wallet_reserved(&trader), 0);
    assert_eq!(t.c().protection(&0), None);
    StellarAssetClient::new(&t.env, &t.token).mint(&trader, &(10 * USDC));
    t.c().buy_protection(&q, &t.sign(&q));
}

#[test]
fn x9_x10_withdrawal_and_amount_errors() {
    let t = setup();
    t.deposit(1_000 * USDC);
    assert_eq!(t.c().try_withdraw_execute(), Err(Ok(Error::NoWithdrawal)));
    assert_eq!(
        t.c().try_withdraw_request(&0),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        t.c().try_withdraw_request(&-1),
        Err(Ok(Error::InvalidAmount))
    );
    t.c().withdraw_request(&(100 * USDC));
    assert_eq!(
        t.c().try_withdraw_request(&(100 * USDC)),
        Err(Ok(Error::WithdrawalPending))
    );
    let lp = t.trader(10 * USDC);
    assert_eq!(
        t.c().try_deposit_capital(&lp, &0),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        t.c().try_deposit_capital(&lp, &-5),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        t.c().try_deposit_capital(&lp, &(11 * USDC)),
        Err(Ok(Error::PaymentFailed))
    );
}

#[test]
fn c1_s6_nonce_cannot_be_reused() {
    let t = setup();
    t.deposit(1_000 * USDC);
    let trader = t.trader(100 * USDC);
    let q = t.quote(&trader, 100 * USDC, 25 * USDC, USDC);
    t.buy(&q).unwrap();
    assert_eq!(t.buy(&q), Err(Error::NonceUsed));

    let other = t.trader(100 * USDC);
    let mut q2 = t.quote(&other, 200 * USDC, 50 * USDC, 2 * USDC);
    q2.nonce = q.nonce;
    assert_eq!(t.buy(&q2), Err(Error::NonceUsed));
    assert_eq!(t.c().state(), state(1_001, 25, 0, 0));
}

#[test]
fn c2_two_traders_race_for_last_capacity() {
    let t = setup();
    t.deposit(100 * USDC);
    let a = t.trader(100 * USDC);
    let b = t.trader(100 * USDC);
    let qa = t.quote(&a, 400 * USDC, 80 * USDC, USDC);
    let qb = t.quote(&b, 400 * USDC, 80 * USDC, USDC);
    t.buy(&qa).unwrap();
    assert_eq!(t.buy(&qb), Err(Error::InsufficientCapital));
    t.assert_invariant();
}

#[test]
fn c3_withdrawal_between_quote_and_buy() {
    let t = setup();
    t.deposit(1_000 * USDC);
    let trader = t.trader(100 * USDC);
    let q = t.quote(&trader, 3_000 * USDC, 600 * USDC, USDC);
    t.c().withdraw_request(&(500 * USDC));
    assert_eq!(t.buy(&q), Err(Error::InsufficientCapital));
    t.buy(&t.quote(&trader, 3_000 * USDC, 501 * USDC, USDC))
        .unwrap();
    t.assert_invariant();
}

#[test]
fn s1_s2_owner_only_withdrawals() {
    let t = setup();
    t.deposit(1_000 * USDC);
    t.env.set_auths(&[]);
    assert!(matches!(
        t.c().try_withdraw_request(&(100 * USDC)),
        Err(Err(_))
    ));
    t.env.mock_all_auths();
    t.c().withdraw_request(&(100 * USDC));
    t.env.set_auths(&[]);
    t.env.ledger().set_timestamp(NOW + WEEK);
    assert!(matches!(t.c().try_withdraw_execute(), Err(Err(_))));
}

#[test]
fn s4_s5_buy_requires_quoted_traders_auth() {
    let t = setup();
    t.deposit(1_000 * USDC);
    let victim = t.trader(100 * USDC);
    let q = t.quote(&victim, 100 * USDC, 25 * USDC, USDC);
    let sig = t.sign(&q);
    t.env.set_auths(&[]);
    assert!(matches!(t.c().try_buy_protection(&q, &sig), Err(Err(_))));
    assert_eq!(t.token().balance(&victim), 100 * USDC);
}

#[test]
fn s8_direct_transfer_does_not_add_capacity() {
    let t = setup();
    t.deposit(100 * USDC);
    let donor = t.trader(1_000 * USDC);
    t.token().transfer(&donor, &t.id, &(1_000 * USDC));
    assert_eq!(t.c().state(), state(100, 0, 0, 0));
    let trader = t.trader(100 * USDC);
    assert_eq!(
        t.buy(&t.quote(&trader, 1_000 * USDC, 200 * USDC, USDC)),
        Err(Error::InsufficientCapital)
    );
}

// Generated by pricer/test/quote.test.ts (FIXTURE). Regenerate both together.
const JS_XDR: &str = "0000001100000001000000120000000f00000006616d6f756e7400000000000a000000000000000000000002540be4000000000f0000000561737365740000000000000f00000003584c4d000000000f00000009646972656374696f6e00000000000003000000000000000f000000086475726174696f6e0000000500000000000151800000000f0000000a657870697265735f617400000000000500000000000f425e0000000f00000003666565000000000a00000000000000000000000001c9c3800000000f0000000a6d61785f7061796f757400000000000a0000000000000000000000009502f9000000000f000000056e6f6e636500000000000005000000000000002a0000000f00000005705f656e640000000000000a0000000000000000000000000016e3600000000f00000007705f7374617274000000000a000000000000000000000000001e84800000000f000000077065727064657800000000120000000103030303030303030303030303030303030303030303030303030303030303030000000f0000000b706f736974696f6e5f6964000000000d0000002000000000000000000000000000000000000000000000000000000000000000000000000f000000077072656d69756d000000000a0000000000000000000000000f485a800000000f00000003717479000000000a00000000000000000000000ba43b74000000000f0000000971756f7465645f61740000000000000500000000000f42400000000f000000067472616465720000000000120000000102020202020202020202020202020202020202020202020202020202020202020000000f000000057661756c74000000000000120000000101010101010101010101010101010101010101010101010101010101010101010000000f00000007766f6c5f627073000000000300002710";
const JS_SIG: &str = "b5bd05b88444d81275d85ddb031c58294b11b2cabe10de94165733bed93c218c88a456ebf6d4eb925dd2b2ba07e66f79229c17839d9ff8317ce3c22a2be3e90e";

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn h5_js_signed_quote_is_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(NOW);
    let at = |c: &str| Address::from_str(&env, c);
    let vault = at("CAAQCAIBAEAQCAIBAEAQCAIBAEAQCAIBAEAQCAIBAEAQCAIBAEAQC526");
    let trader = at("CABAEAQCAIBAEAQCAIBAEAQCAIBAEAQCAIBAEAQCAIBAEAQCAIBAFNSZ");
    let perpdex = at("CABQGAYDAMBQGAYDAMBQGAYDAMBQGAYDAMBQGAYDAMBQGAYDAMBQGCK3");
    let token = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let oracle = env.register(MockOracle, ());
    MockOracleClient::new(&env, &oracle).set(&symbol_short!("XLM"), &NOW, &(XLM * PX));
    env.register_at(&perpdex, MockPerpdex, (oracle.clone(),));
    let position_id = MockPerpdexClient::new(&env, &perpdex).open(
        &trader,
        &symbol_short!("XLM"),
        &tosbi_mock_perpdex::Direction::Long,
        &(5_000 * USDC),
    );
    let lp = Address::generate(&env);
    let cfg = Config {
        admin: lp.clone(),
        owner: lp.clone(),
        operator: lp.clone(),
        token: token.clone(),
        treasury: Address::generate(&env),
        pricer: BytesN::from_array(
            &env,
            &SigningKey::from_bytes(&[7u8; 32])
                .verifying_key()
                .to_bytes(),
        ),
        oracle,
        params: params(10_000),
    };
    let args = (
        cfg,
        vec![&env, symbol_short!("XLM")],
        vec![&env, perpdex.clone()],
    );
    env.register_at(&vault, ProtectionContract, args);
    let sac = StellarAssetClient::new(&env, &token);
    sac.mint(&lp, &(1_000 * USDC));
    sac.mint(&trader, &(100 * USDC));
    let c = ProtectionContractClient::new(&env, &vault);
    c.deposit_capital(&lp, &(1_000 * USDC));

    let q = Quote {
        vault: vault.clone(),
        perpdex,
        position_id,
        trader: trader.clone(),
        asset: symbol_short!("XLM"),
        direction: Direction::Long,
        amount: 10_000_000_000,
        qty: 50_000_000_000,
        p_start: 2_000_000,
        p_end: 1_500_000,
        duration: 86_400,
        max_payout: 2_500_000_000,
        premium: 256_400_000,
        fee: 30_000_000,
        vol_bps: 10_000,
        quoted_at: NOW,
        expires_at: NOW + 30,
        nonce: 42,
    };
    let xdr: Vec<u8> = q.clone().to_xdr(&env).iter().collect();
    assert_eq!(xdr, unhex(JS_XDR));
    let sig: [u8; 64] = unhex(JS_SIG).try_into().unwrap();
    assert_eq!(c.buy_protection(&q, &BytesN::from_array(&env, &sig)), 0);
    assert_eq!(c.state().reserved, 250 * USDC);
}

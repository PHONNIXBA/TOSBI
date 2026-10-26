use super::binding::covered;
use super::*;

#[test]
fn g1_changes_wait_for_the_timelock() {
    let t = setup();
    let new = Params {
        a_min: 50 * USDC,
        ..params(10_000)
    };
    let cid = t.c().propose(&Change::Params(new.clone()));
    assert_eq!(t.env.auths()[0].0, t.admin);
    assert_eq!(t.c().pending_change(&cid).unwrap().eta, NOW + DAY);
    t.env.ledger().set_timestamp(NOW + DAY - 1);
    assert_eq!(t.c().try_execute(&cid), Err(Ok(Error::Timelocked)));
    t.env.ledger().set_timestamp(NOW + DAY);
    t.c().execute(&cid);
    assert_eq!(t.c().config().params, new);
    assert_eq!(t.c().try_execute(&cid), Err(Ok(Error::NoChange)));
}

#[test]
fn g2_admin_can_cancel_and_only_admin_proposes() {
    let t = setup();
    let cid = t.c().propose(&Change::Treasury(Address::generate(&t.env)));
    t.c().cancel(&cid);
    assert_eq!(t.c().pending_change(&cid), None);
    assert_eq!(t.c().try_cancel(&cid), Err(Ok(Error::NoChange)));
    t.env.set_auths(&[]);
    let change = Change::Treasury(Address::generate(&t.env));
    assert!(t.c().try_propose(&change).is_err());
}

#[test]
fn g3_pause_stops_sales_but_not_settlement_or_withdrawals() {
    let t = setup();
    t.deposit(10_000 * USDC);
    let trader = t.trader(1_000 * USDC);
    let id = t
        .buy(&t.quote(&trader, 1_000 * USDC, 250 * USDC, 26 * USDC))
        .unwrap();
    let stranger = Address::generate(&t.env);
    assert_eq!(t.c().try_pause(&stranger), Err(Ok(Error::Unauthorized)));

    t.c().pause(&t.owner);
    assert!(t.c().paused());
    let q = t.quote(&trader, 100 * USDC, 25 * USDC, USDC);
    assert_eq!(t.buy(&q), Err(Error::Paused));
    assert_eq!(t.settle_at(id, 1_700_000), 150 * USDC);
    t.c().withdraw_request(&(100 * USDC));

    t.c().unpause();
    assert_eq!(t.env.auths()[0].0, t.admin);
    assert!(!t.c().paused());
    t.c().pause(&t.operator);
    assert!(t.c().paused());
}

#[test]
fn g4_rotated_pricer_key_replaces_the_old_one() {
    let t = setup();
    t.deposit(1_000 * USDC);
    let trader = t.trader(100 * USDC);
    let new_key = SigningKey::from_bytes(&[9u8; 32]);
    let raw = BytesN::from_array(&t.env, &new_key.verifying_key().to_bytes());
    let cid = t.c().propose(&Change::Pricer(raw));
    t.env.ledger().set_timestamp(NOW + DAY);
    t.c().execute(&cid);
    let q = t.quote(&trader, 100 * USDC, 25 * USDC, USDC);
    assert!(matches!(
        t.c().try_buy_protection(&q, &t.sign(&q)),
        Err(Err(_))
    ));
    t.c().buy_protection(&q, &t.sign_with(&new_key, &q));
}

#[test]
fn g5_assets_can_be_enabled_and_disabled() {
    let t = setup();
    t.deposit(10_000 * USDC);
    let trader = t.trader(1_000 * USDC);
    let on = t.c().propose(&Change::Asset(symbol_short!("ETH"), true));
    let off = t.c().propose(&Change::Asset(symbol_short!("XLM"), false));
    t.env.ledger().set_timestamp(NOW + DAY);
    t.c().execute(&on);
    t.c().execute(&off);
    t.feed("ETH", t.now(), 4_000 * USDC);
    t.buy(&t.quote_for(&trader, "ETH", Direction::Long, 250 * USDC))
        .unwrap();
    let q = t.quote(&trader, 1_000 * USDC, 250 * USDC, USDC);
    assert_eq!(t.buy(&q), Err(Error::AssetDisabled));
}

#[test]
fn g6_only_the_operator_posts_the_vol_hash() {
    let t = setup();
    let h = BytesN::from_array(&t.env, &[1u8; 32]);
    t.c().set_vol_hash(&h);
    assert_eq!(t.env.auths()[0].0, t.operator);
    assert_eq!(t.c().vol_hash(), Some(h));
    t.env.set_auths(&[]);
    let h2 = BytesN::from_array(&t.env, &[2u8; 32]);
    assert!(t.c().try_set_vol_hash(&h2).is_err());
}

#[test]
fn b11_delisted_perpdex_still_settles_early() {
    let t = setup_with(Params {
        timelock: 3_600,
        ..params(10_000)
    });
    t.deposit(10_000 * USDC);
    let trader = t.trader(1_000 * USDC);
    let (pos, id) = covered(&t, &trader);
    let cid = t.c().propose(&Change::Perpdex(t.perpdex.clone(), false));
    t.env.ledger().set_timestamp(NOW + 3_600);
    t.c().execute(&cid);
    let q = t.quote(&trader, 1_000 * USDC, 250 * USDC, USDC);
    assert_eq!(t.buy(&q), Err(Error::PerpdexNotAllowed));

    t.feed("XLM", NOW + 3_600, 1_800_000);
    t.dex().close(&pos, &None);
    assert_eq!(t.c().settle_early(&id), 100 * USDC);
}

#[test]
fn g7_disabling_an_asset_still_settles_live_cover() {
    let t = setup_with(Params {
        timelock: 3_600,
        ..params(10_000)
    });
    t.deposit(10_000 * USDC);
    let trader = t.trader(1_000 * USDC);
    let id = t
        .buy(&t.quote(&trader, 1_000 * USDC, 250 * USDC, USDC))
        .unwrap();
    let cid = t.c().propose(&Change::Asset(symbol_short!("XLM"), false));
    t.env.ledger().set_timestamp(NOW + 3_600);
    t.c().execute(&cid);
    assert_eq!(t.settle_at(id, 1_700_000), 150 * USDC);
}

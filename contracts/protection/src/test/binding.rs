use super::*;

pub(super) const SIZE: i128 = 5_000 * USDC;

pub(super) fn funded() -> (T, Address) {
    let t = setup();
    t.deposit(10_000 * USDC);
    let trader = t.trader(1_000 * USDC);
    (t, trader)
}

/// Buys 1,000 USDC of cover on a fresh XLM long position; returns (position, protection id).
pub(super) fn covered(t: &T, trader: &Address) -> (BytesN<32>, u64) {
    let pos = t.open(trader, "XLM", Direction::Long, SIZE);
    let q = t.quote_on(trader, pos.clone(), 1_000 * USDC, 250 * USDC, 26 * USDC);
    (pos, t.buy(&q).unwrap())
}

#[test]
fn b1_mint_requires_the_traders_matching_open_position() {
    let (t, trader) = funded();
    let other = t.trader(0);
    let buy_on = |pos: BytesN<32>| t.buy(&t.quote_on(&trader, pos, 1_000 * USDC, 250 * USDC, USDC));

    assert_eq!(
        buy_on(t.open(&other, "XLM", Direction::Long, SIZE)),
        Err(Error::PositionMismatch)
    );
    assert_eq!(
        buy_on(t.open(&trader, "XLM", Direction::Short, SIZE)),
        Err(Error::PositionMismatch)
    );
    assert_eq!(
        buy_on(t.open(&trader, "BTC", Direction::Long, SIZE)),
        Err(Error::PositionMismatch)
    );
    assert_eq!(
        buy_on(t.open(&trader, "XLM", Direction::Long, SIZE - 1)),
        Err(Error::PositionMismatch)
    );
    let closed = t.open(&trader, "XLM", Direction::Long, SIZE);
    t.dex().close(&closed, &None);
    assert_eq!(buy_on(closed), Err(Error::PositionMismatch));
    assert_eq!(
        buy_on(BytesN::from_array(&t.env, &[9u8; 32])),
        Err(Error::PositionMismatch)
    );

    assert_eq!(t.c().state(), state(10_000, 0, 0, 0));
    buy_on(t.open(&trader, "XLM", Direction::Long, SIZE)).unwrap();
}

#[test]
fn b2_mark_must_be_within_half_a_percent_of_the_quote() {
    let (t, trader) = funded();
    let pos = t.open(&trader, "XLM", Direction::Long, SIZE);
    t.feed("XLM", NOW, 2_010_001);
    let q = t.quote_on(&trader, pos.clone(), 1_000 * USDC, 250 * USDC, USDC);
    assert_eq!(t.buy(&q), Err(Error::MarkMoved));
    t.feed("XLM", NOW, 2_010_000);
    let q = t.quote_on(&trader, pos, 1_000 * USDC, 250 * USDC, USDC);
    t.buy(&q).unwrap();
}

#[test]
fn b3_position_must_be_at_least_a_minute_old() {
    let t = setup_with(Params {
        min_position_age: 60,
        ..params(10_000)
    });
    t.deposit(10_000 * USDC);
    let trader = t.trader(1_000 * USDC);
    let pos = t.open(&trader, "XLM", Direction::Long, SIZE);
    t.env.ledger().set_timestamp(NOW + 59);
    let q = t.quote_on(&trader, pos.clone(), 1_000 * USDC, 250 * USDC, USDC);
    assert_eq!(t.buy(&q), Err(Error::PositionTooNew));
    t.env.ledger().set_timestamp(NOW + 60);
    let q = t.quote_on(&trader, pos, 1_000 * USDC, 250 * USDC, USDC);
    t.buy(&q).unwrap();
}

#[test]
fn b4_one_protection_per_position_and_allowlists() {
    let (t, trader) = funded();
    let (pos, _) = covered(&t, &trader);
    let q = t.quote_on(&trader, pos, 1_000 * USDC, 250 * USDC, USDC);
    assert_eq!(t.buy(&q), Err(Error::AlreadyCovered));

    t.feed("ETH", t.now(), 4_000 * USDC);
    let q = t.quote_for(&trader, "ETH", Direction::Long, 250 * USDC);
    assert_eq!(t.buy(&q), Err(Error::AssetDisabled));

    let rogue = t.env.register(MockPerpdex, (t.oracle.clone(),));
    let pos = MockPerpdexClient::new(&t.env, &rogue).open(
        &trader,
        &symbol_short!("XLM"),
        &tosbi_mock_perpdex::Direction::Long,
        &SIZE,
    );
    let mut q = t.quote_on(&trader, pos, 1_000 * USDC, 250 * USDC, USDC);
    q.perpdex = rogue;
    assert_eq!(t.buy(&q), Err(Error::PerpdexNotAllowed));
}

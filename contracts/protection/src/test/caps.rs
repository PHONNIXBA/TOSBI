use super::*;

#[test]
fn k1_last_ten_percent_of_the_pool_is_never_sold() {
    let t = setup_with(Params {
        util_cap_bps: 9_000,
        ..params(10_000)
    });
    t.deposit(1_000 * USDC);
    let trader = t.trader(1_000 * USDC);
    // available after the 1 USDC premium = 1,001 → 90% = 900.9
    let cap = 1_001 * USDC * 9 / 10;
    let q = t.quote(&trader, 4_000 * USDC, cap + 1, USDC);
    assert_eq!(t.buy(&q), Err(Error::UtilizationCap));
    let q = t.quote(&trader, 4_000 * USDC, cap, USDC);
    t.buy(&q).unwrap();
    t.assert_invariant();
}

#[test]
fn k2_concentration_caps_per_side_and_per_asset() {
    let t = setup_with(Params {
        side_cap_bps: 2_500,
        asset_cap_bps: 4_000,
        ..params(10_000)
    });
    t.deposit(999 * USDC);
    let trader = t.trader(1_000 * USDC);
    let buy =
        |asset: &str, dir: Direction, max: i128| t.buy(&t.quote_for(&trader, asset, dir, max));
    // Each buy adds its 1 USDC premium to available capital before the check.
    // 250 = 25% of 1,000.
    buy("XLM", Direction::Long, 250 * USDC).unwrap();
    // 251 > 25% of 1,001.
    let over_side = buy("XLM", Direction::Long, USDC);
    assert_eq!(over_side, Err(Error::ConcentrationCap));
    // XLM total 400 ≤ 40% of 1,001.
    buy("XLM", Direction::Short, 150 * USDC).unwrap();
    // XLM total 401 > 40% of 1,002.
    let over_asset = buy("XLM", Direction::Short, USDC);
    assert_eq!(over_asset, Err(Error::ConcentrationCap));
    buy("BTC", Direction::Long, 200 * USDC).unwrap();

    assert_eq!(
        t.c().exposure(&symbol_short!("XLM"), &Direction::Long),
        250 * USDC
    );
    assert_eq!(
        t.c().exposure(&symbol_short!("XLM"), &Direction::Short),
        150 * USDC
    );
    assert_eq!(
        t.c().exposure(&symbol_short!("BTC"), &Direction::Long),
        200 * USDC
    );
    t.assert_invariant();
}

#[test]
fn k4_fee_is_split_with_the_perpdex() {
    let t = setup_with(Params {
        share_pdx_bps: 3_000,
        ..params(10_000)
    });
    t.deposit(10_000 * USDC);
    let trader = t.trader(1_000 * USDC);
    t.buy(&t.quote(&trader, 1_000 * USDC, 250 * USDC, USDC))
        .unwrap();
    // fee = 0.3% of 1,000 = 3 USDC → 0.9 to the PerpDEX owner, 2.1 to the treasury
    assert_eq!(t.token().balance(&t.owner), 9 * USDC / 10);
    assert_eq!(t.token().balance(&t.treasury), 21 * USDC / 10);
}

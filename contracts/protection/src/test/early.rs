use super::binding::{covered, funded, SIZE};
use super::*;

#[test]
fn b5_close_within_tolerance_settles_at_the_close_price() {
    let (t, trader) = funded();
    let (pos, id) = covered(&t, &trader);
    t.env.ledger().set_timestamp(NOW + 3_600);
    t.feed("XLM", NOW + 3_600, 1_810_000);
    assert_eq!(t.c().try_settle_early(&id), Err(Ok(Error::NotClosed)));
    t.dex().close(&pos, &Some(1_800_000));
    assert_eq!(t.c().settle_early(&id), 100 * USDC);
    assert_eq!(t.c().try_settle_early(&id), Err(Ok(Error::AlreadySettled)));
    let p = t.c().protection(&id).unwrap();
    assert_eq!((p.settle_price, p.status), (1_800_000, Status::Settled));
    assert_eq!(t.c().state(), state(10_026 - 100, 0, 0, 0));
}

#[test]
fn b6_outlier_close_falls_back_to_the_oracle() {
    let (t, trader) = funded();
    let (pos, id) = covered(&t, &trader);
    t.env.ledger().set_timestamp(NOW + 3_600);
    t.feed("XLM", NOW + 3_600, 1_810_000);
    t.dex().close(&pos, &Some(1_700_000));
    assert_eq!(t.c().settle_early(&id), 95 * USDC);
    assert_eq!(t.c().protection(&id).unwrap().settle_price, 1_810_000);
}

#[test]
fn b7_close_before_activation_pays_nothing() {
    let (t, trader) = funded();
    let (pos, id) = covered(&t, &trader);
    t.env.ledger().set_timestamp(NOW + 299);
    t.feed("XLM", NOW + 299, 1_000_000);
    t.dex().close(&pos, &None);
    assert_eq!(t.c().settle_early(&id), 0);
    assert_eq!(t.c().state(), state(10_026, 0, 0, 0));
}

#[test]
fn b8_liquidation_settles_like_a_close() {
    let (t, trader) = funded();
    let (pos, id) = covered(&t, &trader);
    t.env.ledger().set_timestamp(NOW + 3_600);
    t.feed("XLM", NOW + 3_600, 1_500_000);
    t.dex().liquidate(&pos, &None);
    assert_eq!(t.c().settle_early(&id), 250 * USDC);
}

#[test]
fn b9_reduced_position_settles_whole_contract_at_the_oracle() {
    let (t, trader) = funded();
    let (pos, id) = covered(&t, &trader);
    t.env.ledger().set_timestamp(NOW + 3_600);
    t.feed("XLM", NOW + 3_600, 1_900_000);
    t.dex().reduce(&pos, &(SIZE - USDC));
    assert_eq!(t.c().settle_early(&id), 50 * USDC);
    t.assert_invariant();
}

#[test]
fn b10_close_after_expiry_uses_the_expiry_twap() {
    let (t, trader) = funded();
    let (pos, id) = covered(&t, &trader);
    let end = NOW + DAY;
    t.feed_window("XLM", end, 1_700_000);
    t.env.ledger().set_timestamp(end + 100);
    t.dex().close(&pos, &Some(1_000_000));
    assert_eq!(t.c().settle_early(&id), 150 * USDC);
}

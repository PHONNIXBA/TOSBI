use super::*;

impl T {
    /// Every record from 35 minutes before `end` through `end`: more than any settlement window.
    pub(super) fn feed_window(&self, asset: &str, end: u64, price: i128) {
        let mut ts = end - 2_100;
        while ts <= end {
            self.feed(asset, ts, price);
            ts += 300;
        }
    }
    /// Moves to expiry if needed, sets every XLM record in the window to `price`, and settles.
    pub(super) fn settle_at(&self, id: u64, price: i128) -> i128 {
        let p = self.c().protection(&id).unwrap();
        if self.now() < p.t_expire {
            self.env.ledger().set_timestamp(p.t_expire);
        }
        self.feed_window("XLM", p.t_expire, price);
        self.c().settle(&id)
    }
}

/// 10,000 USDC pool; trader protects 1,000 USDC of XLM long at $0.20 down to $0.15 for a day.
fn bought(t: &T) -> (Address, u64) {
    t.deposit(10_000 * USDC);
    let trader = t.trader(100 * USDC);
    let id = t
        .buy(&t.quote(&trader, 1_000 * USDC, 250 * USDC, 26 * USDC))
        .unwrap();
    (trader, id)
}

#[test]
fn st1_long_payout_tracks_the_loss_up_to_the_cap() {
    for (price, payout) in [(2_300_000, 0), (1_700_000, 150), (1_000_000, 250)] {
        let t = setup();
        let (trader, id) = bought(&t);
        let before = t.token().balance(&trader);
        assert_eq!(t.settle_at(id, price), payout * USDC, "price {}", price);
        assert_eq!(t.token().balance(&trader) - before, payout * USDC);
        assert_eq!(t.c().state(), state(10_026 - payout, 0, 0, 0));
        let p = t.c().protection(&id).unwrap();
        assert_eq!(
            (p.status, p.settle_price, p.payout),
            (Status::Settled, price, payout * USDC)
        );
        assert_eq!(t.c().wallet_reserved(&trader), 0);
        t.assert_invariant();
    }
}

#[test]
fn st2_short_payout_mirrors_long() {
    for (price, payout) in [(1_700_000, 0), (2_300_000, 150), (3_000_000, 250)] {
        let t = setup();
        t.deposit(10_000 * USDC);
        let trader = t.trader(100 * USDC);
        let pos = t.open(&trader, "XLM", Direction::Short, 5_000 * USDC);
        let mut q = t.quote_on(&trader, pos, 1_000 * USDC, 250 * USDC, 22 * USDC);
        q.direction = Direction::Short;
        q.p_end = 2_500_000;
        let id = t.buy(&q).unwrap();
        assert_eq!(t.settle_at(id, price), payout * USDC, "price {}", price);
        t.assert_invariant();
    }
}

#[test]
fn st3_twap_averages_the_window_and_skips_spikes() {
    let t = setup();
    let (_, id) = bought(&t);
    let end = NOW + DAY;
    t.env.ledger().set_timestamp(end);
    t.feed_window("XLM", end, 1_700_000);
    // A −41% single-record spike inside the window is ignored.
    t.feed("XLM", end - 600, 1_000_000);
    assert_eq!(t.c().settle(&id), 150 * USDC);

    let t = setup();
    let (_, id) = bought(&t);
    t.env.ledger().set_timestamp(end);
    t.feed_window("XLM", end, 1_700_000);
    // A −5.9% record is real movement: (4 × 0.17 + 0.16) / 5 = 0.168.
    t.feed("XLM", end - 600, 1_600_000);
    assert_eq!(t.c().settle(&id), 160 * USDC);
}

#[test]
fn st4_missing_oracle_waits_then_settles_degraded() {
    let t = setup();
    let (_, id) = bought(&t);
    let end = NOW + DAY;
    t.feed("XLM", end - 3_600, 1_700_000);
    t.env.ledger().set_timestamp(end + 60);
    assert_eq!(t.c().try_settle(&id), Err(Ok(Error::OracleStale)));
    t.env.ledger().set_timestamp(end + 7_199);
    assert_eq!(t.c().try_settle(&id), Err(Ok(Error::OracleStale)));

    t.env.ledger().set_timestamp(end + 7_200);
    assert_eq!(t.c().settle(&id), 150 * USDC);
    let last = t.env.events().all().filter_by_contract(&t.id);
    let degraded = Settled {
        id,
        price: 1_700_000,
        payout: 150 * USDC,
        early: false,
        degraded: true,
    };
    assert_eq!(last.events().last(), Some(&degraded.to_xdr(&t.env, &t.id)));
}

#[test]
fn st5_window_scales_with_duration_and_size() {
    let p = params(10_000);
    for (d, w) in [
        (900, 300),
        (3_600, 300),
        (14_400, 492),
        (43_200, 853),
        (86_400, 1_206),
        (259_200, 1_800),
        (604_800, 1_800),
    ] {
        assert_eq!(window_for(d, USDC, &p), w, "duration {}", d);
    }
    assert_eq!(window_for(900, 10_000 * USDC, &p), 300);
    assert_eq!(window_for(900, 10_000 * USDC + 1, &p), 1_800);
}

#[test]
fn st6_late_call_still_uses_the_window_at_expiry() {
    let t = setup();
    let (_, id) = bought(&t);
    let end = NOW + DAY;
    t.feed_window("XLM", end, 1_700_000);
    t.env.ledger().set_timestamp(end + 3 * 3_600);
    t.feed("XLM", end + 3 * 3_600, 2_400_000);
    assert_eq!(t.c().settle(&id), 150 * USDC);
}

#[test]
fn x8_settle_errors() {
    let t = setup();
    t.deposit(1_000 * USDC);
    let trader = t.trader(100 * USDC);
    assert_eq!(t.c().try_settle(&0), Err(Ok(Error::NotFound)));
    let id = t
        .buy(&t.quote(&trader, 100 * USDC, 25 * USDC, USDC))
        .unwrap();
    t.env.ledger().set_timestamp(NOW + DAY - 1);
    assert_eq!(t.c().try_settle(&id), Err(Ok(Error::NotExpired)));
    t.settle_at(id, XLM);
    assert_eq!(t.c().try_settle(&id), Err(Ok(Error::AlreadySettled)));
    assert_eq!(t.c().state(), state(1_001, 0, 0, 0));
}

#[test]
fn c4_settlement_frees_capacity() {
    let t = setup();
    t.deposit(100 * USDC);
    let trader = t.trader(100 * USDC);
    let id = t
        .buy(&t.quote(&trader, 400 * USDC, 100 * USDC, USDC))
        .unwrap();
    let q = t.quote(&trader, 400 * USDC, 100 * USDC, USDC);
    assert_eq!(t.buy(&q), Err(Error::InsufficientCapital));
    t.settle_at(id, XLM);
    t.buy(&t.quote(&trader, 400 * USDC, 100 * USDC, USDC))
        .unwrap();
}

#[test]
fn c5_random_sequences_keep_full_backing() {
    for seed in 1..=20u64 {
        let t = setup();
        let traders: Vec<Address> = (0..4).map(|_| t.trader(1_000_000 * USDC)).collect();
        let mut rng = seed;
        let mut next = |n: u64| {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (rng >> 33) % n
        };
        let mut ids: Vec<u64> = Vec::new();
        for _ in 0..60 {
            let before = t.c().state();
            let ok = match next(5) {
                0 => {
                    let lp = t.trader(10_000 * USDC);
                    t.c()
                        .try_deposit_capital(&lp, &((next(5_000) as i128 + 1) * USDC))
                        .is_ok()
                }
                1 | 2 => {
                    let trader = &traders[next(4) as usize];
                    let amount = (next(10_000) as i128 + 20) * USDC;
                    let max = amount * (next(50) as i128 + 1) / 100;
                    let r = t.buy(&t.quote(trader, amount, max, amount / 50 + 1));
                    if let Ok(id) = r {
                        ids.push(id);
                    }
                    r.is_ok()
                }
                3 if !ids.is_empty() => {
                    let id = ids[next(ids.len() as u64) as usize];
                    let p = t.c().protection(&id).unwrap();
                    if t.now() < p.t_expire {
                        t.env.ledger().set_timestamp(p.t_expire);
                    }
                    t.feed_window("XLM", p.t_expire, 1_000_000 + next(2_000_000) as i128);
                    let ok = t.c().try_settle(&id).is_ok();
                    t.feed("XLM", t.now(), XLM);
                    ok
                }
                _ => {
                    if t.c().state().unlock_at != 0 {
                        t.env.ledger().set_timestamp(t.now() + WEEK);
                        t.c().try_withdraw_execute().is_ok()
                    } else {
                        t.c()
                            .try_withdraw_request(&((next(3_000) as i128 + 1) * USDC))
                            .is_ok()
                    }
                }
            };
            if !ok {
                assert_eq!(
                    t.c().state(),
                    before,
                    "failed call changed state (seed {})",
                    seed
                );
            }
            t.assert_invariant();
        }
    }
}

#[test]
fn s3_settlement_needs_no_signature() {
    let t = setup();
    t.deposit(1_000 * USDC);
    let trader = t.trader(100 * USDC);
    let id = t
        .buy(&t.quote(&trader, 100 * USDC, 25 * USDC, USDC))
        .unwrap();
    t.env.ledger().set_timestamp(NOW + DAY);
    t.feed_window("XLM", NOW + DAY, 1_700_000);
    t.env.set_auths(&[]);
    assert_eq!(t.c().settle(&id), 15 * USDC);
}

#[test]
fn k3_settlement_releases_exposure() {
    let t = setup();
    t.deposit(10_000 * USDC);
    let trader = t.trader(1_000 * USDC);
    let id = t
        .buy(&t.quote(&trader, 1_000 * USDC, 250 * USDC, USDC))
        .unwrap();
    assert_eq!(
        t.c().exposure(&symbol_short!("XLM"), &Direction::Long),
        250 * USDC
    );
    t.settle_at(id, XLM);
    assert_eq!(t.c().exposure(&symbol_short!("XLM"), &Direction::Long), 0);
    t.assert_invariant();
}

#[test]
fn st7_settlement_frees_the_position_for_new_cover() {
    let t = setup();
    t.deposit(10_000 * USDC);
    let trader = t.trader(1_000 * USDC);
    let pos = t.open(&trader, "XLM", Direction::Long, 5_000 * USDC);
    let q = t.quote_on(&trader, pos.clone(), 1_000 * USDC, 250 * USDC, USDC);
    let id = t.buy(&q).unwrap();
    t.settle_at(id, XLM);
    let q = t.quote_on(&trader, pos, 1_000 * USDC, 250 * USDC, USDC);
    t.buy(&q).unwrap();
}

#[test]
fn st8_a_missing_oracle_record_inside_the_window_is_skipped() {
    let t = setup();
    let (_, id) = bought(&t);
    let end = NOW + DAY;
    t.env.ledger().set_timestamp(end);
    // Records every 5 minutes for the 1d window (5 records + anchor), except end − 600.
    for k in 0..=5u64 {
        if k != 2 {
            t.feed("XLM", end - k * 300, 1_700_000);
        }
    }
    assert_eq!(t.c().settle(&id), 150 * USDC);
}

#[test]
fn st9_btc_scale_prices_pay_to_the_unit() {
    let t = setup();
    t.deposit(10_000 * USDC);
    let trader = t.trader(1_000 * USDC);
    // 1,000 USDC of BTC long at $100,000 → 0.01 BTC; 25% range → max payout 250.
    let id = t
        .buy(&t.quote_for(&trader, "BTC", Direction::Long, 250 * USDC))
        .unwrap();
    let p = t.c().protection(&id).unwrap();
    assert_eq!((p.qty, p.p_start), (100_000, 100_000 * USDC));
    t.env.ledger().set_timestamp(p.t_expire);
    // $100,000 → $91,234.5678901: loss 0.087654321 × 1,000 = 87.654321 USDC.
    t.feed_window("BTC", p.t_expire, 912_345_678_901);
    assert_eq!(t.c().settle(&id), 876_543_210);
    t.assert_invariant();
}

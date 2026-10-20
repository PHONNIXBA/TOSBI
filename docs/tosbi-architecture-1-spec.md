# TOSBI — Architecture 1 Build Specification
## Reserve-Pool Put Spread, Fixed Duration Buckets, Full Backing

Version 0.1 — Draft for engineering and risk review
Target chain: Stellar (Soroban). Settlement asset: USDC.

---

## 0. Scope

This document specifies the MVP protection product for TOSBI: a loss-linked, capped protection contract attached to a PerpDEX position, priced off-chain from a volatility table, reserved and settled on-chain, and fully backed by PerpDEX-supplied Protection Capital.

It covers: product terms, pricing formulas, capital rules, settlement rules, anti-abuse rules, Soroban contract architecture, off-chain services, initial risk parameters, a worked example, monitoring, test plan, and references.

Out of scope for v0.1: streaming/perpetual protection (Architecture 3), tranched vaults (Architecture 4), delta-hedging (Architecture 5).

---

## 1. Notation

| Symbol | Meaning | Unit |
|---|---|---|
| `A` | Protected Amount (notional chosen by trader) | USDC |
| `P_start` | Protection Start price (position mark at purchase) | USDC per unit |
| `Q = A / P_start` | Protected quantity | asset units |
| `R` | Coverage Range, chosen by trader | fraction (e.g. 0.25) |
| `P_end` | Coverage End price | USDC per unit |
| `D` | Duration | seconds |
| `τ = D / 31,557,600` | Duration in years (365.25-day year) | years |
| `σ_dir(asset, τ)` | Effective volatility from the vol table, per direction: `σ_down` prices LONG protection, `σ_up` prices SHORT protection | annualized fraction |
| `MaxPayout` | Maximum payout = reserved capital | USDC |
| `Π_fair` | Fair (unloaded) premium | USDC |
| `Π` | Charged premium | USDC |
| `F` | Protection Fee (TOSBI revenue) | USDC |
| `K` | Protection Capital in vault | USDC |
| `U` | Utilization = Σ active MaxPayout / K | fraction |
| `C_{a,d}` | Concentration share of (asset a, direction d) = Σ MaxPayout(a,d) / K | fraction |
| `P_settle` | Settlement price | USDC per unit |
| `W(D)` | Settlement TWAP window | seconds |
| `N(·)` | Standard normal CDF | — |

Direction `d ∈ {LONG, SHORT}` and asset `a` are read from the PerpDEX Position Contract, never entered by the trader.

---

## 2. Product terms

### 2.1 Trader chooses
1. `A` — Protected Amount, `A_min ≤ A ≤ min(A_max, position notional)`.
2. `D` — Duration, from the bucket menu `{15m, 1h, 4h, 12h, 1d, 3d, 7d}`. Off-menu durations are permitted in v0.2 via interpolation (§3.4); v0.1 restricts to the menu.
3. `R` — Coverage Range, `R_min ≤ R ≤ R_max` (initial 0.05–0.50).

### 2.2 TOSBI determines
```
P_start   = mark price from Position Contract at quote time
P_end     = P_start × (1 − R)          if LONG
          = P_start × (1 + R)          if SHORT
MaxPayout = Q × |P_start − P_end| = A × R
Premium   = Π  (see §3)
Fee       = F  (see §3.7)
Reserved  = MaxPayout
```

### 2.3 Payout at settlement
```
LONG:   Payout = min( Q × max(0, P_start − P_settle),  MaxPayout )
SHORT:  Payout = min( Q × max(0, P_settle − P_start),  MaxPayout )
```
Equivalently `Payout = A × min(R, max(0, loss_fraction))` where
```
LONG:   loss_fraction = (P_start − P_settle) / P_start
SHORT:  loss_fraction = (P_settle − P_start) / P_start
```

This is a bull put spread (LONG protection) or bear call spread (SHORT protection) held long by the trader.

---

## 3. Pricing

### 3.1 Black–Scholes building blocks

With risk-free rate `r` (set `r = 0` for MVP; USDC carry is negligible at these tenors):

```
d1(K) = [ ln(S/K) + (r + σ²/2) τ ] / (σ √τ)
d2(K) = d1(K) − σ √τ

Put(K)  = K e^{−rτ} N(−d2) − S N(−d1)
Call(K) = S N(d1) − K e^{−rτ} N(d2)
```
where `S = P_start`.

Reference: Black & Scholes (1973); Hull, *Options, Futures and Other Derivatives*, ch. 15.

### 3.2 Fair premium (per contract)

```
LONG:   Π_fair = Q × [ Put(P_start)  − Put(P_end)  ]      using σ = σ_down(asset, τ)
SHORT:  Π_fair = Q × [ Call(P_start) − Call(P_end) ]      using σ = σ_up(asset, τ)
```
The two directions use different volatilities because crypto returns are asymmetric: downside moves are sharper and fatter-tailed than upside moves. With a single σ, an ATM put spread and an ATM call spread would price identically under Black–Scholes (r = 0), which would misprice one side.

Note: because `P_start` is at-the-money at purchase, `Put(P_start)` dominates; `Put(P_end)` is small for short tenors and moderate `R`, but must be computed because it matters for 3–7 day covers and for small `R`.

Useful ATM approximation for sanity checks (r = 0): `Put_ATM ≈ 0.3989 × S × σ√τ`.

### 3.3 Volatility table

`σ_dir(asset, τ_bucket)` is maintained off-chain, one value per (asset, bucket, direction) — a `σ_down` calibrated on falls and a `σ_up` calibrated on rises.

**Do not derive short-bucket σ by scaling annualized vol with √τ.** Short-horizon crypto returns are fat-tailed; a √τ-scaled diffusion under-prices 15-minute to 4-hour covers (Matic, Packham & Härdle 2023; Du 2025). Instead calibrate each bucket empirically:

For bucket `τ_b`, collect the rolling history of `τ_b`-horizon log returns of the asset over the trailing window `H_b` (initial: 30 days for ≤ 12h buckets, 90 days for ≥ 1d). Define the **quantile-matched σ** as the σ such that the Black–Scholes put spread price equals the empirical expected payoff:

```
down_τ = (P_0 − P_τ) / P_0 = 1 − exp(ret_τ)        loss fraction for a LONG over horizon τ_b
up_τ   = (P_τ − P_0) / P_0 = exp(ret_τ) − 1        loss fraction for a SHORT over horizon τ_b

σ_down,b = argmin_σ | [Put_BS(S,σ,τ_b)  − Put_BS(S(1−R_ref),σ,τ_b) ] / S  −  E_emp[ max(0, min(R_ref, down_τ)) ] |
σ_up,b   = argmin_σ | [Call_BS(S,σ,τ_b) − Call_BS(S(1+R_ref),σ,τ_b)] / S  −  E_emp[ max(0, min(R_ref, up_τ)) ] |
```
Both sides are expressed as a fraction of Protected Amount. Expect `σ_down > σ_up` for most assets in most regimes; do not force them equal. With `R_ref = 0.25` (or solve at 2–3 reference ranges and take the max). This absorbs jump risk into σ without a separate jump model.

Additionally enforce a floor: `σ_dir,b ≥ σ_floor(asset)` (initial: 60% for XLM, 50% for BTC/ETH, same floor both directions) and a cap `σ_dir,b ≤ 300%`.

Refresh cadence: every 4 hours for buckets ≤ 12h, daily for ≥ 1d. Re-price immediately on a "vol shock" trigger (realized 1h vol > 2× table σ_1h).

**v1.1 upgrade:** replace flat-per-bucket σ with SVI in log-moneyness `k = ln(K/S)`:
```
w(k) = a + b [ ρ (k − m) + sqrt( (k − m)² + s² ) ],   σ²τ = w(k)
```
subject to Gatheral–Jacquier no-arbitrage constraints, calibrated to Deribit/Bybit BTC/ETH surfaces and to empirical skew for XLM. This gives correct pricing for `Put(P_end)` at large `R`.

### 3.4 Off-bucket durations (v0.2)

Interpolate in **total variance**, not in σ:
```
w_dir,b = σ_dir,b² τ_b        (interpolate each direction separately)
w(τ) = w_lo + (w_hi − w_lo) × (τ − τ_lo)/(τ_hi − τ_lo)      for τ_lo ≤ τ ≤ τ_hi
σ(τ) = sqrt( w(τ) / τ )
```
Require `w(τ)` non-decreasing in `τ` (calendar no-arbitrage); if violated, bump `w_hi` up.

Optional long-term path: map `D → q` and price with the Amortizing Perpetual Option closed form (Feinstein 2025), which prices all durations from one formula. Note the provisional US patent on AmPOs before adopting.

### 3.5 Risk loadings

Charged premium:
```
Π = Π_fair × M_u × M_c × (1 + ρ)
```

**Utilization loading** (capacity pricing):
```
U' = (Σ active MaxPayout + MaxPayout_new) / K
M_u = 1 + λ_u × U'^p_u
```
Initial `λ_u = 0.5`, `p_u = 2`. At `U' = 0.4`, `M_u = 1.08`; at `U' = 0.9`, `M_u = 1.405`.

**Concentration loading** (same asset, same direction):
```
C' = (Σ MaxPayout(a,d) + MaxPayout_new) / K
M_c = 1 + λ_c × C'^p_c
```
Initial `λ_c = 1.0`, `p_c = 2`. At `C' = 0.2`, `M_c = 1.04`; at the `C' = 0.25` cap, `M_c = 1.0625` — the hard cap (§4.2) binds before the loading gets large, which is intentional.

**Risk margin** `ρ`: flat safety margin over fair value. Initial `ρ = 0.05`.

Rationale: options market makers charge a premium that rises with net inventory imbalance (Alexander et al., "Net Buying Pressure and the Information in Bitcoin Option Trades"). Actuarially these are variance/standard-deviation premium principles (Kaas et al., *Modern Actuarial Risk Theory*, ch. 5).

### 3.6 Minimum premium

```
Π = max( Π, Π_min )
Π_min = max( 0.5 USDC, 0.10% × A )
```
Prevents near-zero quotes on very short, deep-range covers.

### 3.7 Protection Fee

```
F = max( F_min, f_bps × A )
```
Initial `F_min = 0.5 USDC`, `f_bps = 30 bp`. Fee goes to TOSBI treasury; revenue share with the PerpDEX is a config parameter `share_pdx ∈ [0,1]`, initial 0.

### 3.8 Total cost to trader
```
Cost = Π + F
```
`Π` is transferred into the ProtectionVault (it belongs to the capital providers). `F` is transferred to TOSBI treasury.

---

## 4. Capital rules

### 4.1 Full backing (invariant, enforced on-chain)
```
Σ_{active contracts} MaxPayout_i  ≤  K_available
K_available = vault_balance − pending_payouts − withdrawal_locked
```
Mint fails if the invariant would be violated.

### 4.2 Hard caps (enforced on-chain)
| Cap | Initial value |
|---|---|
| Per (asset, direction) concentration `C_{a,d}` | ≤ 25% of K |
| Per asset (both directions) | ≤ 40% of K |
| Per trader wallet, sum of active MaxPayout | ≤ 2% of K and ≤ 25,000 USDC |
| Per contract `A_max` | 50,000 USDC |
| Per contract `A_min` | 20 USDC |
| Global utilization `U` | ≤ 90% (last 10% is buffer for oracle/settlement slippage) |

### 4.3 Liquidity-relative cap (enforced off-chain by pricer, checked on-chain via param)
```
Σ protected notional(a) ≤ κ × Depth_a
```
where `Depth_a` = USDC needed to move asset `a` by `R_ref` across the venues the oracle reads (estimated daily). Initial `κ = 0.5`. Rationale: a manipulator must spend more than they can extract (Mackinga, Nadahalli & Wattenhofer 2022).

### 4.4 PerpDEX capital withdrawal
Withdrawals are queued: requested amount becomes `withdrawal_locked` immediately (blocks new mints against it) and is releasable after `T_withdraw` (initial 7 days) provided the full-backing invariant still holds. This mirrors senior-tranche withdrawal delays in DeFi insurance (Nadler, Bekemeier & Schär 2022).

### 4.5 Premium accrual
Premiums enter the vault immediately and count toward `K` — they increase capacity. Track `premium_earned` and `payout_paid` per epoch for loss-ratio monitoring (§9).

---

## 5. Settlement

### 5.1 Normal settlement (expiry)
```
P_settle = TWAP_oracle(asset, window = W(D), ending at T_expire)
```

**Window function:**
```
W(D) = clamp( 300 s,  0.53 × sqrt(D_minutes) × 60 s ,  1800 s )
```
Equivalently: `W = clamp(5 min, 0.53 × √(D in minutes) min, 30 min)`.

| Duration | W(D) |
|---|---|
| 15m | 5 min (floor) |
| 1h | 5 min (floor) |
| 4h | ~8 min |
| 12h | ~14 min |
| 1d | ~20 min |
| 3d | 30 min (cap) |
| 7d | 30 min (cap) |

Rationale: manipulation cost should scale with contract value; longer windows for longer (typically larger) covers. 30-minute cap matches Bybit Perp Protect and Deribit convention.

**Size override:** if `MaxPayout > 10,000 USDC`, use `W = 1800 s` regardless of duration.

Settlement may be triggered by anyone (keeper or trader) at any time `t ≥ T_expire`; the contract reads the TWAP for the window `[T_expire − W, T_expire]`. Because Reflector's TWAP is over the last N records, the keeper must call within one oracle resolution of expiry; if it is later, the contract uses the closest available window and emits a `LateSettlement` event (see §5.4).

### 5.2 Early settlement (position closed)
When the PerpDEX Position Contract reports the position closed at `P_close` at time `t_close < T_expire`:

```
P_ref = lastprice_oracle(asset) at t_close (or nearest record)
if | P_close − P_ref | / P_ref ≤ δ_close:
    P_settle = P_close
else:
    P_settle = P_ref            # do not trust an outlier close
```
Initial `δ_close = 1.5%`.

Rationale: prevents a trader from wash-trading a bad close price on a thin book to inflate payout.

**Partial close:** if the position size falls below `Q`, protection continues on `Q' = min(Q, remaining size)` and `MaxPayout` is reduced pro-rata; excess reserve is released. (v0.1: settle the whole contract on any close; partial handling in v0.2.)

### 5.3 Liquidation of the underlying position
Treated as early settlement with `P_close = liquidation price` reported by the Position Contract, subject to the same `δ_close` check. Protection survives liquidation (matches Bybit).

### 5.4 Oracle safety checks (on-chain)
```
staleness:  reject settlement if oracle.last_timestamp < now − 2 × oracle.resolution; retry later
deviation:  if secondary oracle available and |P_primary − P_secondary|/P_secondary > δ_oracle (2%):
               P_settle = the value closer to the pre-window 1h TWAP
per-record cap: ignore any single record deviating > 15% from the prior record
                (truncated-oracle pattern; see Uniswap v4 truncated oracle)
```

### 5.5 Settlement timing edge cases
- If the oracle is stale for longer than `T_stale_max` (initial 2 hours), settlement uses the last valid TWAP and emits `DegradedSettlement`; admin may pause new mints.
- Contract expiry aligned to oracle resolution boundaries to avoid off-by-one window issues.

---

## 6. Anti-abuse rules

| Rule | Value | Purpose |
|---|---|---|
| Position binding | trader must hold open position ≥ `Q` on the same asset/direction at mint | prevents "close the long, keep the put" |
| Activation delay | protection starts at `t_mint + 300 s`; `P_start` fixed at mint | stops buying at the instant a move begins |
| Quote TTL | signed quote valid 30 s | prevents stale-price arbitrage |
| Min duration since position open | 60 s | discourages open-protect-close loops |
| One active contract per position per direction | v0.1 | simplifies settlement; multiple in v0.2 |
| Event surcharge | pricer applies `M_event ≥ 1` inside configured windows (unlocks, macro prints) | informed-flow pricing |
| Vol-shock repricing | if realized 1h vol > 2× table, pricer bumps σ and pauses ≤ 1h buckets for 15 min | avoids selling into a crash at stale prices |
| Per-wallet cap | §4.2 | limits single informed player |

The premium loadings (§3.5) are the primary economic defense; the rules above close mechanical loopholes.

---

## 7. Soroban contract architecture

### 7.1 Contracts

```
tosbi-registry        params, allowlisted PerpDEX adapters, pause, admin (multisig)
tosbi-vault           Protection Capital accounting, reserve/release, payout, withdrawal queue
tosbi-book            mint / settle protection contracts, per-contract state
tosbi-quote-verifier  verifies TOSBI pricer signature over quote payload
tosbi-oracle-adapter  wraps SEP-40 oracle(s): lastprice, twap, decimals, resolution, safety checks
perpdex-adapter-*     one per PerpDEX: reads Position Contract, normalizes fields
```

### 7.2 Quote flow

1. Frontend requests quote from TOSBI pricer with `(perpdex, position_id, A, D, R)`.
2. Pricer reads position (asset, direction, mark), vault state (`K`, `U`, `C`), vol table; computes `Π`, `F`, `P_start`, `P_end`, `MaxPayout`.
3. Pricer signs payload:
```
Quote {
  perpdex: Address, position_id: BytesN<32>, trader: Address,
  asset: Symbol, direction: u32,
  A: i128, Q: i128, P_start: i128, P_end: i128,
  D: u64, MaxPayout: i128, premium: i128, fee: i128,
  vol_used: u32, quoted_at: u64, expires_at: u64, nonce: u64
}
sig = ed25519(pricer_key, sha256(Quote))
```
4. Trader submits `book.buy(quote, sig)`.
5. On-chain: verify signature, `expires_at ≥ now`, nonce unused; re-read position from adapter (must be open, size ≥ Q, asset/direction match, |mark − P_start| ≤ δ_quote 0.5%); check full-backing invariant and caps against current vault state; transfer `premium + fee` from trader; `vault.reserve(MaxPayout)`; write contract record; emit `Minted`.

The on-chain re-check of caps means the pricer's `M_u`/`M_c` may be slightly stale (30 s), which is acceptable: the invariant is enforced on-chain regardless.

### 7.3 Contract record (persistent storage)
```rust
struct Protection {
    id: u64,
    trader: Address,
    perpdex: Address,
    position_id: BytesN<32>,
    asset: Symbol,
    direction: Direction,        // Long | Short
    q: i128,                     // units, 7 decimals
    p_start: i128,               // oracle decimals
    p_end: i128,
    max_payout: i128,            // USDC 7 decimals
    premium_paid: i128,
    fee_paid: i128,
    t_mint: u64,
    t_active: u64,               // t_mint + activation delay
    t_expire: u64,
    window_secs: u32,
    status: Status,              // Pending | Active | Settled | Cancelled
    settle_price: Option<i128>,
    payout: Option<i128>,
}
```

### 7.4 Storage & TTL
- `Protection` records, vault balances, reserve totals, per-(asset,direction) totals, per-wallet totals: **persistent** storage.
- Nonce set: persistent, pruned after settlement.
- Quote payloads: not stored (verified and discarded).
- Extend TTL on every write and on every read in `settle`; keeper extends TTL for all active records every 24h. Target TTL ≥ `D + 14 days`.
- Never use temporary storage for anything that affects money.

### 7.5 Keeper
Off-chain service that: (a) calls `settle(id)` at `t_expire` within one oracle resolution; (b) watches PerpDEX close events and calls `settle_early(id)`; (c) extends TTLs; (d) posts vol-table hashes for auditability. Anyone may call `settle` — keeper is a liveness convenience, not a trust dependency.

### 7.6 Required PerpDEX Position Contract interface
```rust
fn get_position(position_id: BytesN<32>) -> Position {
    owner: Address, asset: Symbol, direction: Direction,
    size: i128, entry_price: i128, mark_price: i128,
    status: PositionStatus,          // Open | Closed | Liquidated
    close_price: Option<i128>, close_ts: Option<u64>,
}
event PositionClosed(position_id, close_price, close_ts, reason)
```
If a PerpDEX cannot expose `close_price` on-chain, the adapter falls back to oracle-only early settlement (`P_settle = P_ref`).

### 7.7 Oracle adapter (SEP-40)
Uses: `lastprice(asset)`, `twap(asset, records)`, `decimals()`, `resolution()`. `records = ceil(W / resolution)`. Read `decimals()` at runtime; do not hard-code. Primary: Reflector. Secondary (deviation check only): Pyth or RedStone on Stellar. Verify on-chain that Reflector's `twap` covers the intended window; if `resolution` is 5 min, the 5-min floor equals one record — for ≤1h buckets consider Reflector's paid faster feed or accept single-record settlement with the per-record cap.

### 7.8 Events
`Minted`, `Activated`, `Settled{price, payout}`, `SettledEarly`, `LateSettlement`, `DegradedSettlement`, `CapitalDeposited`, `WithdrawalRequested`, `WithdrawalExecuted`, `ParamsUpdated`, `Paused`.

### 7.9 Admin & upgrades
Multisig admin (TOSBI + PerpDEX signers). Parameter changes time-locked 24h except `pause`. Contract upgrade via Soroban WASM upgrade behind the same time-lock.

---

## 8. Off-chain services

| Service | Responsibility |
|---|---|
| Pricer | vol table, loadings, quote signing; stateless HTTP; hot key in HSM/KMS |
| Vol calibrator | empirical bucket σ (§3.3), SVI (v1.1), event calendar, vol-shock detection |
| Keeper | settlement, early settlement, TTL extension |
| Risk monitor | utilization, concentration, loss ratio, oracle health, alerts; can trigger pause |
| Depth estimator | daily `Depth_a` for liquidity cap (§4.3) |

Publish the vol table and its hash on-chain (or IPFS + on-chain hash) per refresh so PerpDEX partners can audit quotes.

---

## 9. Initial risk parameters

| Parameter | Value | Section |
|---|---|---|
| Buckets | 15m, 1h, 4h, 12h, 1d, 3d, 7d | 2.1 |
| `R_min`, `R_max` | 5%, 50% | 2.1 |
| `r` | 0 | 3.1 |
| `σ_floor` | XLM 60%, BTC/ETH 50% (both directions) | 3.3 |
| Vol table key | (asset, bucket, direction) | 3.3 |
| `λ_u`, `p_u` | 0.5, 2 | 3.5 |
| `λ_c`, `p_c` | 1.0, 2 | 3.5 |
| `ρ` | 5% | 3.5 |
| `Π_min` | max(0.5 USDC, 0.10% A) | 3.6 |
| `F` | max(0.5 USDC, 30 bp × A) | 3.7 |
| Concentration cap | 25% per (asset,dir), 40% per asset | 4.2 |
| Wallet cap | 2% of K, ≤ 25k USDC | 4.2 |
| `A_min`, `A_max` | 20, 50,000 USDC | 4.2 |
| Global `U` cap | 90% | 4.2 |
| `κ` | 0.5 | 4.3 |
| `T_withdraw` | 7 days | 4.4 |
| `W(D)` | clamp(5m, 0.53√D_min, 30m) | 5.1 |
| `δ_close` | 1.5% | 5.2 |
| `δ_oracle` | 2% | 5.4 |
| `δ_quote` | 0.5% | 7.2 |
| Activation delay | 300 s | 6 |
| Quote TTL | 30 s | 6 |

KPIs to tune these: loss ratio (§10), utilization distribution, quote-to-mint conversion.

---

## 10. Worked example

### 10.1 LONG protection

XLM LONG, `P_start = 0.20`, `A = 1,000 USDC` → `Q = 5,000 XLM`, `D = 1d`, `R = 25%` → `P_end = 0.15`, `MaxPayout = 250`.

Vol table: `σ_down,1d(XLM) = 100%`. `τ = 1/365.25 = 0.002738`, `σ√τ = 0.05232`.

ATM put (r = 0): `d1 = σ√τ/2 = 0.02616`, `d2 = −0.02616`.
`Put(0.20) = 0.20 × [N(0.02616) − N(−0.02616)] = 0.20 × 0.02087 = 0.004174` per unit.
`Put(0.15)`: `ln(0.20/0.15) = 0.2877`, `d2 = (0.2877 − 0.00137)/0.05232 ≈ 5.47` → `N(−5.47) ≈ 2×10⁻⁸` → negligible.

`Π_fair = 5,000 × 0.004174 = 20.87 USDC` (2.09% of A).

Vault state: `K = 500,000`, active reserve 249,750 → `U' = (249,750 + 250)/500,000 = 0.50` → `M_u = 1 + 0.5 × 0.25 = 1.125`.
XLM-long reserve 99,750 → `C' = 0.20` → `M_c = 1 + 1.0 × 0.04 = 1.04` (below the 25% cap in §4.2).

`Π = 20.87 × 1.125 × 1.04 × 1.05 = 25.64 USDC`.
`F = max(0.5, 0.003 × 1,000) = 3.00 USDC`.
**Total cost = 28.64 USDC (2.86% of A).**

Settlement window: `W = 0.53 × √1440 ≈ 20 min`.

Outcomes at expiry:
- `P_settle = 0.23` → payout 0; trader net −28.64.
- `P_settle = 0.17` → `5,000 × 0.03 = 150` → trader receives 150; net −28.64 after offsetting a 150 position loss.
- `P_settle = 0.10` → `5,000 × 0.10 = 500` capped at 250 → trader receives 250.
- Early close at `P_close = 0.18` with oracle at 0.181 (dev 0.55% ≤ 1.5%) → `5,000 × 0.02 = 100`.

Vault economics for this contract: +25.64 premium in, 250 reserved for 1 day, expected payout ≈ 20.87 (by construction of σ) → expected margin ≈ 4.77 + loadings' cushion.

### 10.2 SHORT protection

XLM SHORT, `P_start = 0.20`, `A = 1,000 USDC` → `Q = 5,000 XLM`, `D = 1d`, `R = 25%` → `P_end = 0.25`, `MaxPayout = 250`.

Vol table: `σ_up,1d(XLM) = 90%` (calibrated on rises; lower than the 100% downside figure).

`σ√τ = 0.90 × 0.05232 = 0.04709`.

ATM call (r = 0): `d1 = σ√τ/2 = 0.02355`, `d2 = −0.02355`.
`Call(0.20) = 0.20 × [N(0.02355) − N(−0.02355)] = 0.20 × 0.01879 = 0.003757` per unit.
`Call(0.25)`: `ln(0.20/0.25) = −0.2231`, `d1 = (−0.2231 + 0.00111)/0.04709 ≈ −4.71` → `N(−4.71) ≈ 1.2×10⁻⁶` → negligible.

`Π_fair = 5,000 × 0.003757 = 18.79 USDC` (1.88% of A).

Vault state: `K = 500,000`, active reserve 249,750 → `U' = 0.50` → `M_u = 1.125`.
XLM-short reserve 49,750 → `C' = 0.10` → `M_c = 1 + 1.0 × 0.01 = 1.01`.

`Π = 18.79 × 1.125 × 1.01 × 1.05 = 22.41 USDC`.
`F = 3.00 USDC`.
**Total cost = 25.41 USDC (2.54% of A).**

Outcomes at expiry:
- `P_settle = 0.17` → payout 0; trader net −25.41 (position gained 150).
- `P_settle = 0.23` → `5,000 × 0.03 = 150` → trader receives 150; net −25.41 after offsetting a 150 position loss.
- `P_settle = 0.30` → `5,000 × 0.10 = 500` capped at 250 → trader receives 250.
- Early close at `P_close = 0.22` → `5,000 × 0.02 = 100`.

The short costs less than the long on the same terms (25.41 vs 28.64) for two reasons: the upside vol is lower, and short protection is less crowded in this vault state, so its concentration loading is smaller.

---

## 11. Monitoring

**Loss ratio (primary KPI):**
```
LR_epoch = Σ payouts / Σ premiums   (epoch = 7 days, also 30 days rolling)
```
Targets: `LR ≤ 0.80` rolling 30d. If `LR > 1.0` for two consecutive weeks → raise `ρ` or bucket σ. If `LR < 0.4` for 8 weeks with low utilization → lower `ρ`/loadings to improve conversion.

Per-bucket and per-asset LR to detect which tenors are mispriced (expect short buckets to be the problem first).

Others: utilization time series; max concentration; oracle staleness incidents; settlement latency; number of `δ_close` overrides (signals wash-trading attempts); quote-to-mint conversion by price level (demand elasticity).

---

## 12. Test & validation plan

1. **Pricing backtest:** for XLM/BTC/ETH, 2022–2026 hourly data; simulate selling every bucket × R combination daily; compute realized LR under the empirical-σ calibration; confirm `LR < 1` over the full period and inspect worst 30-day windows (May 2022, Nov 2022, Aug 2024, Oct 2025, Feb 2026).
2. **Crash stress:** instantaneous −30% and −50% with fully concentrated book at cap; verify vault solvency and that payouts equal Σ MaxPayout.
3. **Oracle attack simulation:** inject single-record spikes of +15%/−15%/−40% into the TWAP window; confirm per-record cap and deviation logic bound payout error to < `δ_oracle`.
4. **Wash-close test:** close position at ±3% from oracle; verify `δ_close` fallback.
5. **Invariant fuzzing:** randomized mint/settle/withdraw sequences; assert full-backing invariant and cap invariants after every step.
6. **TTL test:** 7-day contract with no interaction; verify keeper extension and archival-restore path.
7. **Latency:** keeper settles within one oracle resolution of expiry ≥ 99% of the time on testnet.
8. **Audit:** apply to Soroban Audit Bank via SCF Build.

---

## 13. Roadmap after v0.1

- v0.2: off-bucket durations (variance interpolation), partial close, multiple contracts per position.
- v1.1: SVI surface; secondary oracle deviation check live; event calendar surcharges.
- v2: tranched vault (PerpDEX first-loss junior + public senior) — Architecture 4.
- v2.x: streaming/AmPO product for open-ended protection — Architecture 3 (resolve patent question first).
- Later: partial delta-hedging once a Stellar perp venue has deep mainnet liquidity — Architecture 5.

---

## 14. References

**Option pricing & volatility**
- Black, F. & Scholes, M. (1973). The Pricing of Options and Corporate Liabilities. *Journal of Political Economy* 81(3).
- Hull, J. C. *Options, Futures, and Other Derivatives* (11th ed.), ch. 15 (Black–Scholes–Merton), ch. 12 (spreads).
- Gatheral, J. & Jacquier, A. (2014). Arbitrage-free SVI volatility surfaces. *Quantitative Finance* 14(1). https://arxiv.org/abs/1204.0646
- Du, L. (2025). Pricing Cryptocurrency Options With Volatility of Volatility. *Journal of Futures Markets*. https://onlinelibrary.wiley.com/doi/10.1002/fut.70029
- Matic, J. L., Packham, N. & Härdle, W. K. (2023). Hedging Cryptocurrency Options. *Review of Derivatives Research* 26(1). https://arxiv.org/abs/2112.06807
- Alexander, C. & Imeraj, A. (2023). Delta hedging bitcoin options with a smile. *Quantitative Finance* 23(5). https://www.tandfonline.com/doi/full/10.1080/14697688.2023.2181205
- Alexander, C., Deng, J., Feng, J. & Wan, H. (2021). Net Buying Pressure and the Information in Bitcoin Option Trades. https://arxiv.org/abs/2109.02776
- Zbandut, A. (2025). Pricing of wrapped Bitcoin and Ethereum on-chain options. https://arxiv.org/abs/2512.20190

**Perpetual / streaming designs (for §3.4 and roadmap)**
- Feinstein, Z. (2025). Amortizing Perpetual Options. https://arxiv.org/abs/2512.06505
- Bichuch, M. & Feinstein, Z. (2026). Designing On-Chain Options: Amortizing Perpetual Options. https://arxiv.org/abs/2605.19146
- Lambert, G. & Kristensen, J. (2022). Panoptic: the perpetual, oracle-free options protocol. https://arxiv.org/abs/2204.14232
- Ackerer, D., Hugonnier, J. & Jermann, U. (2026). Perpetual Futures Pricing. *Mathematical Finance*. https://arxiv.org/abs/2310.11771
- Angeris, G., Chitra, T., Evans, A. & Lorig, M. (2023). A primer on perpetuals. *SIAM J. Financial Mathematics* 14(1).

**Insurance capital & pricing**
- Kaas, R., Goovaerts, M., Dhaene, J. & Denuit, M. *Modern Actuarial Risk Theory* (2nd ed.), ch. 5 (premium principles).
- Nadler, M., Bekemeier, F. & Schär, F. (2022). DeFi Risk Transfer: Towards A Fully Decentralized Insurance Protocol. https://arxiv.org/abs/2212.10308
- Singh, S. F., Michalopoulos, P. & Veneris, A. (2024). BAKUP: Automated, Flexible, and Capital-Efficient Insurance Protocol for DeFi. https://arxiv.org/abs/2410.09341
- Bekemeier, F., Schär, F. & Schmeiser, H. (2025). Decentralized Finance risk transfer and smart contract-based insurance. *Journal of Banking & Finance*. https://www.sciencedirect.com/science/article/pii/S0378426625002262

**Oracle manipulation & settlement**
- Mackinga, T., Nadahalli, T. & Wattenhofer, R. (2022). TWAP Oracle Attacks: Easier Done than Said? IACR ePrint 2022/445. https://eprint.iacr.org/2022/445.pdf
- Chaos Labs. Uniswap V3 TWAP: Assessing TWAP Market Risk. https://chaoslabs.xyz/posts/chaos-labs-uniswap-v3-twap-market-risk
- Uniswap v4 Truncated Oracle (Hacken analysis). https://hacken.io/discover/uniswap-v4-truncated-oracle/

**Industry references (UX and settlement conventions)**
- Bybit. Perp Protect help pages. https://www.bybit.com/en/help-center/article/Perp-Protect ; https://www.bybit.com/en/learn/bybit-guide/contract-insurance
- Nami Insurance docs. https://docs.namiinsurance.io/ (glossary, status system, futures-position cover, conditional contracts)
- Bumper. https://www.bumper.fi/

**Stellar / Soroban**
- SEP-40 Oracle interface. https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0040.md
- Reflector Network docs and contract. https://reflector.network/docs ; https://github.com/reflector-network/reflector-contract
- Stellar oracle providers. https://developers.stellar.org/docs/data/oracles/oracle-providers
- Soroban state archival / TTL. https://developers.stellar.org/docs/learn/fundamentals/contract-development/storage/state-archival
- Stellar Community Fund (Build awards, Audit Bank). https://stellar.org/grants-and-funding
- Circle USDC on Stellar. https://www.circle.com/multi-chain-usdc/stellar

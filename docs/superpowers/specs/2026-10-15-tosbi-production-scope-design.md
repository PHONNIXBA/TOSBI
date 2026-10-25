# TOSBI — Production Scope Across SOW 1–3 (Design)

Date: 2026-10-15 · Status: draft for review
Inputs: [SOW 1](../../tosbi-instaward-1-sow.md), [Architecture 1 spec](../../tosbi-architecture-1-spec.md), [Business overview](../../tosbi-architecture-1-business-overview.md), current code at `2e0beb2`.

---

## 1. Goal

After the three Instawards, TOSBI is **mainnet-ready with the audit pending**:

- The contract implements every v0.1 function in the Architecture 1 spec, is hardened, frozen at a tagged WASM hash, and submitted to the Soroban Audit Bank.
- Off-chain services (pricer, keeper, calibrator, risk monitor, indexer) run unattended on testnet with alerting.
- A TypeScript SDK and partner integration kit are published; real users have run the full flow on testnet.
- A mainnet deploy runbook exists and has been dry-run on testnet.

Not in the three SOWs (SCF Build Award): the audit itself, mainnet deployment, real USDC, legal review, v0.2 (any duration, partial close, multiple protections per position), v1.1 (SVI, second oracle, event surcharges), tranched vault.

## 2. Constraints

| Constraint | Effect |
|---|---|
| Instawards: $5k each, $15k total, at most two follow-ons | SOW 3 is the last Instaward. Audit cost is out of reach; Audit Bank via SCF. |
| 10-day windows per SOW | SOW 1 grows to roughly 3× its original scope; slip order defined in §5. |
| SOW 1 submitted 2026-09-13 and its original scope is built (`2e0beb2`) | Expanding SOW 1 needs resubmission through the chapter lead. |
| Launch PerpDEX in talks, no contract deployed | Build against the §7.6 interface with a reference mock Position Contract; partner implements §7.6. |
| Assets: XLM, BTC, ETH | Per-asset vol tables, oracle mapping and exposure tracking from SOW 1. |

## 3. SOW structure

| SOW | Theme | Rule |
|---|---|---|
| 1 | All functions | Every contract entry point and off-chain service exists and works on testnet. |
| 2 | Edge cases, testing, hardening | No new features. Fixes may change the contract. Ends with a frozen release tag and the Audit Bank application. |
| 3 | Real user integration, SDK | Off-chain only. The audited contract does not change, so the audit runs in parallel with SOW 3. |

## 4. Contract architecture

One core contract holds vault, book, quote verification, roles and parameters. The spec's §7.1 split into registry / vault / book / verifier / oracle-adapter is not used: the full-backing invariant and caps are checked in the same call that moves money, and splitting them adds cross-contract trust boundaries and audit surface for no gain while there is one book per vault.

- **Oracle** is a module inside the core contract that calls Reflector through SEP-40. The oracle address is a timelocked parameter.
- **PerpDEX** positions are read by calling `get_position` on an allowlisted contract implementing §7.6. No adapter contract is written; the partner implements §7.6. If they cannot, an adapter contract is a follow-up and needs its own audit.
- **One vault instance per PerpDEX**, each with its own owner. No multi-tenancy.
- **Early settlement** is triggered by the keeper (contracts cannot subscribe to events) and verified by the contract re-reading the position. Anyone may call it.

### 4.1 Roles

| Role | Holder | Can do |
|---|---|---|
| `owner` | PerpDEX (capital provider) | deposit, `withdraw_request` / `withdraw_execute` (paid to `owner`), `pause` |
| `operator` | TOSBI | `pause`, `set_vol_hash` |
| `admin` | Stellar multisig account with TOSBI + PerpDEX signers (native account thresholds, no custom multisig contract) | `propose` / `execute` / `cancel` timelocked changes, `unpause` |

Timelocked changes (24h): risk parameters, enabled assets, pricer key, oracle address, PerpDEX allowlist, fee split `share_pdx`, treasury, WASM upgrade. Pause stops new mints only; settlement, claims and withdrawals keep working.

### 4.2 Final entry points (frozen at end of SOW 2)

```
__constructor(admin, owner, operator, token, treasury, pricer, oracle, params)

deposit_capital(from, amount)
withdraw_request(amount)            owner
withdraw_execute()                  owner
buy_protection(quote, sig)          trader
settle(id)                          anyone, t ≥ t_expire
settle_early(id)                    anyone, position closed/liquidated/reduced
claim(id)                           trader, payout recorded as owed     (SOW 2)

propose(change) -> change_id        admin
execute(change_id)                  admin, ≥ 24h after propose
cancel(change_id)                   admin
pause(caller)                       owner or operator
unpause()                           admin
set_vol_hash(hash)                  operator

state(), config(), protection(id), wallet_reserved(trader),
exposure(asset, direction), pending_change(id)
```

`release` is removed; only settlement frees a reservation. No TTL entry point: every persistent write extends to 30 days, and SOW 2 tests the archival path.

### 4.3 Accounting

```
token_balance = capital + owed
reserved ≤ 0.9 × (capital − locked)                     global utilization cap
exposure(a, d) ≤ 0.25 × (capital − locked)              per asset and direction
exposure(a, Long) + exposure(a, Short) ≤ 0.40 × (capital − locked)
wallet_reserved(t) ≤ min(0.02 × (capital − locked), 25,000)
reserved = Σ max_payout of Active protections = Σ exposure = Σ wallet_reserved
```

At settlement: `capital −= payout`, `reserved`, `exposure`, `wallet_reserved −= max_payout`. If the payout transfer fails, `owed += payout` and the trader calls `claim` later.

## 5. SOW 1 — All functions

| # | Deliverable | Contents |
|---|---|---|
| 1 | Settlement | `settle(id)`. TWAP over `[t_expire − W(D), t_expire]` built from Reflector records, ignoring any record > 15% from the prior one. `W(D) = clamp(5m, 0.53·√D_min, 30m)`, 30m when `max_payout > 10,000`. Records are read one by one with SEP-40 `price(asset, timestamp)`, so a late call still settles on the exact window while the oracle keeps history. Oracle decimals read at runtime. No record at the end of the window → `OracleStale`, retry. Past `t_expire + 2h` with still no record → settle on the last price, `Settled` event with `degraded: true`. Payout per spec §2.3, rounded down. |
| 2 | Position binding + early settlement | Quote gains `perpdex`, `position_id`. Mint checks: DEX allowlisted, position Open, owner = trader, asset and direction match, size ≥ Q, `|mark − p_start| ≤ 0.5%`, `now − open_ts ≥ 60s`, no other active protection on the position. `settle_early(id)`: close or liquidation price, checked against oracle within 1.5%, else the oracle price; oracle-only if the DEX gives no close price. Reference mock Position Contract with `open`, `close`, `liquidate`. |
| 3 | Risk caps, multi-asset | On-chain exposure per (asset, direction); all caps in §4.3 enforced at mint. Pricer reads exposure for the concentration loading. XLM/BTC/ETH vol tables, floors 60% / 50% / 50%. |
| 4 | Governance | Roles and timelock in §4.1. Fee split at mint: `fee × share_pdx` to owner, rest to treasury. |
| 5 | Services | **Keeper** (Worker cron, 1 min): settle expired, `settle_early` when a position is closed, liquidated or reduced (polled). No TTL extension needed: every write extends to 30 days and a protection lives at most 7 days. **Recalibration**: every 4h for all buckets (GitHub Actions; Coinbase candles, since Binance blocks both Cloudflare and US runners); tables to Worker KV, hash posted with `set_vol_hash`. **Vol-shock**: realized 1h vol > 2× table σ_1h → bump σ, refuse ≤1h buckets for 15 min. **Depth estimator**: daily, feeds the liquidity cap. |
| 6 | Demo + tests | Full lifecycle on the demo: open mock position → buy → settle / close early / liquidate → payout receipt, plus each rejection. Happy path and main rejections tested per new function. |

Slip order if 10 days is not enough: depth estimator, then scheduled recalibration (manual run instead). The contract does not depend on either.

### 5.1 Business-logic decisions

| Decision | Reason |
|---|---|
| Position closed before `t_active` settles with zero payout, no refund | Otherwise a trader buys, watches 5 minutes, and backs out for free. |
| Pause stops new mints only | Traders already covered must still be paid. |
| Liquidity cap enforced by the pricer only, on max-payout exposure per asset (`exposure(a) ≤ κ × Depth_a`, κ = 0.5) | A manipulator's gain is bounded by max payout, so that is the right quantity to compare to cost of moving the price. On-chain enforcement would put a daily value behind a 24h timelock, and a stolen pricer key can misprice anyway. |
| §7.6 `Position` gains `open_ts: u64` | The 60-second rule cannot be checked without it. The partner must expose it. |
| Position size reduced below Q while still Open → `settle_early` settles the whole contract at the oracle price | v0.1 has no partial-close handling. Without this, a trader closes most of the position and keeps full protection. |
| PerpDEX allowlist gates new mints only | Removing a DEX must not strip early settlement from its live protections. |
| P_start is the position's mark price, not a CEX spot price | Spec §2.2. The demo's Coinbase/Kraken/Binance spot feed is used only for vol-shock detection. |

### 5.2 Verified on testnet (2026-10-16)

- Reflector testnet `CCYOZJCOPG34LLQQ7N24YXBM7LL62R7ONMZ3G6WZAAYPB5OYKOMJRN63`: XLM, BTC and ETH as `Other(symbol)`, 14 decimals, 300 s resolution, timestamps in seconds, base USD.
- `price(asset, timestamp)` returns the record at the 5-minute boundary at or before `timestamp`; history reaches back about 18–23 hours. A keeper that is hours late still settles on the exact expiry window; past that, the 2-hour degraded rule applies.

## 6. SOW 2 — Edge cases, testing, hardening

| # | Deliverable | Contents |
|---|---|---|
| 1 | Edge-case matrix + fixes | Every (state × event) pair listed and tested; see §6.1. |
| 2 | Invariant fuzzing | Random sequences of deposit, buy, settle, settle_early, claim, withdraw, pause, parameter changes. After each step assert every line of §4.3. |
| 3 | Adversarial simulations | Oracle record spikes ±15% and −40% inside the window; wash closes ±3% from oracle; −30% / −50% crash with the book at its caps; forged, replayed and cross-vault quotes; settle spam. |
| 4 | Pricing validation | Backtest XLM/BTC/ETH hourly 2022–2026, every bucket × range sold daily. Loss ratio < 1 over the full period; report worst windows (May 2022, Nov 2022, Aug 2024, Oct 2025, Feb 2026); tune parameters. |
| 5 | Ops hardening | Pricer key in a managed KMS. Quote API rate limits. Keeper ≥ 99% of settlements within one oracle resolution over 7 days. Untouched 7-day contract survives TTL. Risk monitor: loss ratio (7d, 30d, per bucket and asset), utilization, concentration, oracle staleness, settlement latency, `δ_close` overrides; alerts and auto-pause via `operator`. CI: `cargo test`, clippy, `npm test`, typecheck, reproducible WASM hash. |
| 6 | Audit package | Threat model, invariants document, interface spec, known-issues list, frozen release tag with WASM hash, Audit Bank application. |

### 6.1 Edge cases (seed list)

- Payout transfer fails (trustline removed, issuer freeze) → payout recorded as `owed`, settlement completes, trader uses `claim`.
- `settle` before expiry, `settle` twice, `settle_early` on an Open full-size position, close at or after `t_expire` (routes to `settle`).
- Close before `t_active`; liquidation with an outlier price; DEX gives no close price.
- Oracle stale, then recovers; stale past 2h (degraded); single-record spike inside the window.
- DEX removed from allowlist with live protections; pricer key rotated with a quote in flight (fails, acceptable at 30s TTL).
- Pause with live protections; `withdraw_request` racing buys; parameter change executed while protections are live (applies to new mints only).
- Rounding: payouts round down; the 7 ↔ 14 decimal rescale happens in one function. (No i128 overflow: worst case is ≈10²⁶.)
- Archived protection record or nonce restored after TTL expiry.

## 7. SOW 3 — Real user integration, SDK

| # | Deliverable | Contents |
|---|---|---|
| 1 | TypeScript SDK (npm) | `quote`, `buy`, `settle`, `settleEarly`, `claim`, `getProtection`, `listProtections(trader)`, `vaultState`, event subscription; contract errors mapped to typed errors; browser and Node; built on generated `stellar contract bindings`; docs and examples. |
| 2 | Partner integration kit | §7.6 interface (with `open_ts`); conformance test suite the partner runs against its Position Contract; reference Protect button and Protected badge; integration guide. |
| 3 | Indexer + read API | Stellar RPC keeps events ~7 days. Worker cron → D1. Serves trader history, vault stats, loss ratio. The SOW 1 keeper's active-protection list moves onto it. |
| 4 | Partner integration | Partner testnet contract available → integrate end to end. Not available → integrate against the mock; the conformance suite shows any partner can plug in. |
| 5 | Real-user pilot (testnet) | 20–50 external wallets (Stellar Vietnam community) run the full flow. Report: quote-to-mint conversion, settlement timing, UX issues found and fixed. |
| 6 | Mainnet readiness kit | Deploy script with mainnet values (Circle USDC SAC, Reflector mainnet), dry-run on testnet; dashboards; incident runbooks (oracle outage, pause, key rotation, owed payouts); launch checklist. |

## 8. Deviations from the Architecture 1 spec

| Spec | This design |
|---|---|
| §7.1 six contracts | One core contract; oracle in-contract; no PerpDEX adapter contract unless the partner cannot implement §7.6. |
| §7.6 `Position` | Adds `open_ts`. |
| §4.3 liquidity cap on protected notional, checked on-chain | On max-payout exposure, pricer only. |
| §5.2 partial close settles whole contract (v0.1) | Same, and a size drop below Q while Open also triggers it, at the oracle price. |
| §4.1 `pending_payouts` | Implemented as `owed` + `claim`. |
| §7.9 multisig admin | Stellar native multisig account as `admin`, plus separate `owner` and `operator` roles. |
| (not specified) close before activation | Zero payout, no refund. |

Existing deviations from SOW 1 (canonical-XDR signature, `vault` in the quote, 7-decimal prices) stay as listed in the README.

## 9. Risks

| Risk | Mitigation |
|---|---|
| SOW 1 at ~3× scope in 10 days | Slip order in §5; contract work first. |
| SOW 1 resubmission not accepted | Fall back: SOW 1 as submitted; SOW 2 takes the functions, SOW 3 takes hardening + SDK; integration pilot moves to the Build Award. |
| Partner cannot expose §7.6 or `open_ts` | Adapter contract, separately audited. |
| Reflector testnet lacks a feed, or history is short | Checked (§5.2): all three feeds exist, about 18 hours of history. |
| Binance blocks Cloudflare and US servers | Calibration moved to Coinbase candles, run from GitHub Actions. |
| Selling an option-like product to retail | Legal review before mainnet, under the Build Award. |

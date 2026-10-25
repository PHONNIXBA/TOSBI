# SOW 1 (expanded) — All Functions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the Instaward 1 vault into a feature-complete v0.1 on testnet: settlement on the oracle, position binding and early settlement, on-chain risk caps, governance, multi-asset pricing, a keeper, and a demo that exercises all of it.

**Architecture:** One Soroban contract (`contracts/protection`) holds the vault, book, quote check, oracle reads and governance. Two small fixture contracts stand in for the outside world: `mock-perpdex` (a PerpDEX implementing the position interface) and `mock-oracle` (a SEP-40 oracle for tests and the local network; testnet uses Reflector). The Cloudflare Worker in `pricer/` signs quotes, serves the demo, and runs the keeper on a one-minute cron.

**Tech Stack:** Rust + soroban-sdk 29.0.0, Stellar CLI 26.1, TypeScript on Node 24 (`node --test`, `tsc`), @stellar/stellar-sdk 17, Cloudflare Workers (wrangler 4, KV, cron triggers), GitHub Actions.

**Spec:** [docs/superpowers/specs/2026-10-15-tosbi-production-scope-design.md](../specs/2026-10-15-tosbi-production-scope-design.md) (what and why), with formulas from [docs/tosbi-architecture-1-spec.md](../../tosbi-architecture-1-spec.md). Read both before Task 1.

**Answer key:** [2026-10-16-sow1-answer-key.patch](2026-10-16-sow1-answer-key.patch). This is a working, tested version of every task, built against commit `2e0beb2`. You copy the **tests** out of it and write the **implementation** yourself. See "How to work through this plan".

## Global Constraints

- soroban-sdk `29.0.0` (workspace `Cargo.toml`), Stellar CLI `26.1`, Rust ≥ `1.84` (`u64::isqrt`), Node `24`.
- Every amount and price inside the contract has **7 decimals** (`UNIT = 10_000_000`). Reflector prices have 14 decimals and are converted in exactly one function, `oracle::to_7`.
- Payouts round **down**: `qty * gap / UNIT`, then capped at `max_payout`.
- Error codes are a public API. The numbering in Task 1 is final, and `pricer/src/chain.ts` `ERRORS` must list the same names at the same indexes.
- The `Quote` field set is a public API. The contract checks the pricer's signature over the quote's canonical XDR, so `pricer/src/quote.ts` `TYPES` must have exactly the same fields.
- `Position`, `PositionStatus` and `Direction` must be identical, field for field and number for number, in `contracts/protection/src/position.rs` and `contracts/mock-perpdex/src/lib.rs`.
- Never use temporary storage for anything that touches money. Persistent writes go through `set_persistent`, which extends TTL to 30 days.
- Anyone may call `settle` and `settle_early`. Neither may call `require_auth`.
- Pause stops new sales only. Settlement and withdrawals must keep working while paused.
- Testnet parameters come from `scripts/deploy.sh`: activation 300 s, minimum position age 60 s, timelock 300 s, wallet cap 2%, utilization cap 90%, side cap 25%, asset cap 40%.

## Review Focus

Things the spec implies that are easy to get wrong. Each has a test in the task that owns it:

1. **The oracle skips one 5-minute record inside the settlement window.** Settlement should average the records that do exist, not wait for 2 hours. Test: `st8_a_missing_oracle_record_inside_the_window_is_skipped` (Task 2).
2. **BTC-scale prices (≈ $100,000) with tiny quantities (0.01 BTC).** The payout must be right to the last USDC unit. Test: `st9_btc_scale_prices_pay_to_the_unit` (Task 2).
3. **An asset is disabled while protections on it are live.** Those protections must still settle and pay. Test: `g7_disabling_an_asset_still_settles_live_cover` (Task 4).
4. **The keeper fails to read one position (an RPC hiccup).** It logs the error, settles the others, and retries next minute. Test: `a failed position read is logged and the next protection is still handled` (Task 7).
5. **A demo user switches wallets in Freighter.** The page must not list the previous wallet's positions, because a quote on them builds a transaction the new wallet can't sign. Saved ids are keyed by vault and wallet; there is a manual check in Task 11.

---

## How to work through this plan

You are implementing; your lead reviews each task before you start the next.

1. Work on a branch: `git checkout -b sow1-all-functions` from `main`.
2. Each task starts by **copying its tests out of the answer key** with `git apply --include=...` (the exact command is in the task). Read every test before writing code. The tests are the definition of done, and their names say what they prove.
3. Run the tests and watch them fail, usually because the code doesn't compile yet.
4. Write the implementation from the **Rules** in the task. Rules give the order of checks, the exact error for each case, and the formula. Write it yourself; that is how you'll learn the codebase.
5. Run the tests until they pass, then run the lint/format step, then commit.
6. **Stuck for more than 30 minutes on one step?** Read that file's version in the answer key (`less docs/superpowers/plans/2026-10-16-sow1-answer-key.patch`, then search for the file path). Then close it and write your own. Tell your lead which part you looked at; that's useful, not a failure.
7. Don't apply implementation files from the patch, except the ones a task explicitly tells you to apply (fixtures and interface definitions).

Useful commands:

```sh
cargo test -p tosbi-protection                 # all contract tests
cargo test -p tosbi-protection st1             # tests whose name contains "st1"
cargo fmt && cargo clippy --tests              # format + lint (must print no warnings)
stellar contract build                         # build the .wasm files
cd pricer && npm test && npm run typecheck     # pricer tests + types
```

## Background you need

**What the product does.** A trader holds a perpetual position (for example, long $1,000 of XLM at $0.20). They buy *protection*: if the price falls, the vault pays their loss down to a floor (for example, 25% → $0.15), capped at a *max payout* ($250). The PerpDEX's capital in the vault backs every payout. The **full-backing rule**: the sum of all live max payouts can never exceed the vault's free capital.

**The pieces:**

| Piece | Where | Job |
|---|---|---|
| Vault contract | `contracts/protection` | Holds capital, sells protection against a signed quote, settles, pays |
| Pricer | `pricer/src/service.ts` | Prices protection (Black–Scholes spread + loadings) and signs a `Quote` |
| Oracle | Reflector on testnet, `mock-oracle` locally | SEP-40 price feed: 5-minute records, 14 decimals |
| PerpDEX | `mock-perpdex` | Holds the trader's position; the vault reads it with `get_position` |
| Keeper | `pricer/src/keeper.ts` | Every minute, calls `settle` / `settle_early` on whatever is due |
| Demo | `pricer/public/index.html` | Lets a reviewer do all of the above with Freighter |

**Lifecycle of one protection:**

```
open position on PerpDEX ──► pricer quotes it (mark price, vol table, vault state) ──► trader signs buy_protection
   │                                                                                        │
   │                                           reserve max_payout, take premium + fee ◄─────┘
   │
   ├─ position closed / liquidated / reduced before expiry ──► settle_early: close price (checked vs oracle) or oracle price
   └─ nothing happens until expiry ─────────────────────────► settle: average oracle price over a window ending at expiry
                                                               payout = min(qty × loss per unit, max_payout)
```

**Units:**

| Value | Unit | Example |
|---|---|---|
| amount, max_payout, premium, fee, capital | USDC × 10⁷ | 250 USDC = `2_500_000_000` |
| p_start, p_end, mark, close and settle prices | USD per unit × 10⁷ | $0.20 = `2_000_000`; $100,000 = `1_000_000_000_000` |
| qty, position size | asset units × 10⁷ | 5,000 XLM = `50_000_000_000` |
| oracle prices (Reflector, mock-oracle) | × 10¹⁴ | converted by `oracle::to_7` |
| times | Unix seconds | |

**Soroban basics you'll use.** `#[contracttype]` makes a struct or enum storable and passable. `#[contracterror]` makes an error enum whose numbers are the API. `#[contractclient]` on a trait generates a client to call another contract. Its `try_` methods return `Result` instead of trapping, which is how the vault survives a bad PerpDEX. `env.register(Contract, (args,))` deploys a contract inside a test. `env.mock_all_auths()` makes every `require_auth()` pass in tests, and `env.set_auths(&[])` turns that off to check auth is required. A contract function that returns `Err` rolls back every write and transfer it made.

## File map

| File | Status | Responsibility |
|---|---|---|
| `contracts/mock-oracle/` | new (fixture, applied) | SEP-40 oracle with a public `set` |
| `contracts/mock-perpdex/` | new (fixture, applied) | PerpDEX positions: `open`, `get_position`, `close`, `liquidate`, `reduce` |
| `contracts/protection/src/lib.rs` | rewrite | Types, errors, capital, `buy_protection`, `settle`, `settle_early`, views |
| `contracts/protection/src/position.rs` | new (interface, applied) | §7.6 position types + client |
| `contracts/protection/src/oracle.rs` | new | SEP-40 client, TWAP, decimal conversion |
| `contracts/protection/src/governance.rs` | new | Timelocked changes, pause, vol hash |
| `contracts/protection/src/test.rs` + `test/*.rs` | rewrite/new (applied) | Tests, one file per task |
| `pricer/src/quote.ts` | edit | Quote gains `perpdex`, `position_id` |
| `pricer/src/service.ts` | rewrite | Quote from a position; caps; vol-shock; liquidity cap |
| `pricer/src/chain.ts` | edit | New error names; `invokeAs` for the keeper |
| `pricer/src/keeper.ts`, `pricer/src/market.ts` | new | Keeper loop; realized vol + order-book depth |
| `pricer/src/worker.ts`, `pricer/wrangler.jsonc` | rewrite | API routes, cron handler, KV |
| `pricer/scripts/calibrate.ts`, `publish-vol.ts`, `pricer/vol-tables/*.json` | rewrite/new | Coinbase calibration for 3 assets; publish + hash |
| `.github/workflows/recalibrate.yml` | new | 4-hourly recalibration |
| `scripts/deploy.sh`, `scripts/admin.sh`, `pricer/scripts/e2e.ts` | rewrite/new | Deploy, 2-of-2 admin calls, end-to-end run |
| `pricer/public/index.html` | rewrite (body + script) | Demo |
| `README.md` | edit | Run instructions, deliverables, testnet IDs |

---

### Task 0: Check your environment

- [ ] **Step 1: Branch and confirm the starting point passes**

```sh
git checkout main && git pull && git checkout -b sow1-all-functions
cargo test                                  # expect: 27 passed
cd pricer && npm install && npm test        # expect: 16 pass
npm run typecheck && cd ..
```

- [ ] **Step 2: Confirm the answer key applies**

```sh
git apply --check docs/superpowers/plans/2026-10-16-sow1-answer-key.patch && echo ok
```

Expected: `ok`. If it fails, `main` has moved since `2e0beb2`; ask your lead before going on.

- [ ] **Step 3: Confirm Docker and the local network (needed in Task 9)**

```sh
export DOCKER_HOST=$(docker context inspect --format '{{.Endpoints.docker.Host}}')
stellar container start local --protocol-version 29
curl -s -X POST -H 'content-type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' http://localhost:8000/rpc
stellar container stop local
```

Expected: `"status":"healthy"`. Both flags matter on this Mac. Without `DOCKER_HOST` the CLI can't find Docker, and without `--protocol-version 29` contracts built with soroban-sdk 29 won't deploy.

---

### Task 1: New contract shape, fixtures, position binding and caps

**Goal:** `buy_protection` sells only against the trader's own open position on an allowlisted PerpDEX, enforces every cap, and splits the fee. `release` is gone, and withdrawals belong to the PerpDEX owner.

**Files:**
- Apply (fixtures + interface): `contracts/mock-oracle/**`, `contracts/mock-perpdex/**`, `contracts/protection/src/position.rs`, `contracts/protection/Cargo.toml`
- Apply (tests): `contracts/protection/src/test.rs`, `contracts/protection/src/test/binding.rs`, `contracts/protection/src/test/caps.rs`
- Modify: `contracts/protection/src/lib.rs`

**Interfaces (produced; later tasks rely on these exact names):**

```rust
#[contracterror] #[repr(u32)]
pub enum Error {
    InvalidAmount = 1, InvalidQuote = 2, QuoteExpired = 3, NonceUsed = 4, Unauthorized = 5,
    AmountOutOfRange = 6, InsufficientCapital = 7, WalletCapExceeded = 8, NotFound = 9,
    AlreadySettled = 10, WithdrawalPending = 11, NoWithdrawal = 12, WithdrawalLocked = 13,
    PaymentFailed = 14, Paused = 15, AssetDisabled = 16, PerpdexNotAllowed = 17,
    PositionMismatch = 18, MarkMoved = 19, PositionTooNew = 20, AlreadyCovered = 21,
    UtilizationCap = 22, ConcentrationCap = 23, NotExpired = 24, NotClosed = 25,
    OracleStale = 26, Timelocked = 27, NoChange = 28,
}

pub enum Direction { Long = 0, Short = 1 }      // add: fn opposite(self) -> Self
pub enum Status { Active = 0, Settled = 1 }       // Released is gone

pub struct Params {
    pub a_min: i128, pub a_max: i128, pub wallet_cap_bps: u32, pub wallet_cap_max: i128,
    pub activation_delay: u64, pub withdraw_delay: u64,
    pub util_cap_bps: u32,      // 9000 = 90% of free capital may be reserved
    pub side_cap_bps: u32,      // 2500 = one (asset, direction)
    pub asset_cap_bps: u32,     // 4000 = one asset, both directions
    pub mark_tol_bps: u32,      // 50 = quote p_start within 0.5% of the position's mark
    pub close_tol_bps: u32,     // 150 (used in Task 3)
    pub min_position_age: u64,  // 60 s
    pub record_cap_bps: u32,    // 1500 (Task 2)
    pub stale_max: u64,         // 7200 s (Task 2)
    pub big_payout: i128,       // above this max payout, settle on the full 30-min window
    pub share_pdx_bps: u32,     // fee share paid to the owner
    pub timelock: u64,          // Task 4
}

pub struct Config {
    pub admin: Address, pub owner: Address, pub operator: Address, pub token: Address,
    pub treasury: Address, pub pricer: BytesN<32>, pub oracle: Address, pub params: Params,
}

pub struct Quote {   // field order doesn't matter for XDR (keys are sorted), names do
    pub vault: Address, pub perpdex: Address, pub position_id: BytesN<32>, pub trader: Address,
    pub asset: Symbol, pub direction: Direction, pub amount: i128, pub qty: i128,
    pub p_start: i128, pub p_end: i128, pub duration: u64, pub max_payout: i128,
    pub premium: i128, pub fee: i128, pub vol_bps: u32, pub quoted_at: u64,
    pub expires_at: u64, pub nonce: u64,
}

pub struct Protection {
    pub id: u64, pub trader: Address, pub perpdex: Address, pub position_id: BytesN<32>,
    pub asset: Symbol, pub direction: Direction, pub qty: i128, pub p_start: i128, pub p_end: i128,
    pub max_payout: i128, pub premium: i128, pub fee: i128, pub t_mint: u64, pub t_active: u64,
    pub t_expire: u64, pub window: u64, pub status: Status,
    pub settle_price: i128, pub payout: i128,   // 0 until settled
}

enum Key {   // private
    Config, Vault, NextId, NextChange, Withdrawal, Paused, VolHash,
    Asset(Symbol), Perpdex(Address), Exposure(Symbol, Direction),
    Protection(u64), Covered(Address, BytesN<32>), WalletReserved(Address), Nonce(u64), Change(u64),
}

pub fn __constructor(env: Env, config: Config, assets: Vec<Symbol>, perpdexes: Vec<Address>)
pub fn buy_protection(env: Env, quote: Quote, sig: BytesN<64>) -> Result<u64, Error>
pub fn withdraw_request(env: Env, amount: i128) -> Result<u64, Error>   // owner
pub fn withdraw_execute(env: Env) -> Result<i128, Error>                 // owner, pays owner
pub fn exposure(env: Env, asset: Symbol, direction: Direction) -> i128
pub fn next_id(env: Env) -> u64
pub fn paused(env: Env) -> bool
pub fn window_for(duration: u64, max_payout: i128, p: &Params) -> u64   // crate-level pub fn
// unchanged: deposit_capital, state, config, protection, wallet_reserved
```

`Key` already lists the variants Tasks 2–4 use, so later tasks don't touch it.

- [ ] **Step 1: Copy in the fixtures, interface, and tests**

```sh
KEY=docs/superpowers/plans/2026-10-16-sow1-answer-key.patch
git apply --include='contracts/mock-oracle/*' --include='contracts/mock-perpdex/*' \
  --include='contracts/protection/Cargo.toml' --include='contracts/protection/src/position.rs' \
  --include='contracts/protection/src/test.rs' --include='contracts/protection/src/test/binding.rs' \
  --include='contracts/protection/src/test/caps.rs' $KEY
```

Then open `contracts/protection/src/test.rs` and **delete** the lines `mod early;`, `mod governance;` and `mod settle;` near the top. Tasks 2–4 add them back. Read `mock-perpdex/src/lib.rs` and `position.rs` all the way through: those are the contract you'll be calling.

- [ ] **Step 2: Run the tests and confirm they fail to compile**

Run: `cargo test -p tosbi-protection`
Expected: compile errors such as `cannot find type Config`, `no method named exposure`, `missing field perpdex`.

- [ ] **Step 3: Implement**

Add `mod position;` and `pub use position::{Position, PositionStatus};` at the top of `lib.rs`. Add the constants `UNIT = 10_000_000`, `MIN_WINDOW = 300`, `MAX_WINDOW = 1_800`. Replace the types with the ones above.

Rules for `__constructor`: store `config`; store `Vault::default()` and `NextId = 0`; for every asset store `Key::Asset(a) = true`, and for every PerpDEX `Key::Perpdex(d) = true`. Use instance storage for all of these.

Rules for `buy_protection`, **in this order** (tests check which error wins):

1. Load config. `quote.trader.require_auth()`. Verify `ed25519_verify(pricer, quote.to_xdr, sig)`. These three are unchanged.
2. `check_quote` (new private fn):
   - paused → `Paused`
   - `now > expires_at` → `QuoteExpired`
   - wrong vault, `premium ≤ 0`, `fee < 0`, `max_payout ≤ 0`, `max_payout > amount`, `duration == 0`, `qty ≤ 0` or `p_start ≤ 0` → `InvalidQuote`
   - amount outside `[a_min, a_max]` → `AmountOutOfRange`
   - asset flag not set → `AssetDisabled`
3. Nonce already stored → `NonceUsed`.
4. `Key::Covered(perpdex, position_id)` exists → `AlreadyCovered`.
5. `check_capacity` returns `(Vault, side, wallet)`. Work on a copy of the vault:
   - `capital += premium`, `reserved += max_payout`, `available = capital − locked`
   - `reserved > available` → `InsufficientCapital`
   - `reserved > available × util_cap_bps / 10000` → `UtilizationCap`
   - `side = exposure(asset, dir) + max_payout`; `both = side + exposure(asset, dir.opposite())`
   - `side > available × side_cap_bps / 10000`, or `both > available × asset_cap_bps / 10000` → `ConcentrationCap`
   - wallet check exactly as today (`min(available × wallet_cap_bps / 10000, wallet_cap_max)`) → `WalletCapExceeded`
6. `check_position` comes **last** because it's the only call to another contract:
   - PerpDEX not allowlisted → `PerpdexNotAllowed`
   - `position::get` returns `None`, or status ≠ Open, owner ≠ trader, asset or direction differs, or `size < qty` → `PositionMismatch`
   - `|mark_price − p_start| × 10000 > p_start × mark_tol_bps` → `MarkMoved` (exactly 0.5% is allowed)
   - `now < open_ts + min_position_age` → `PositionTooNew`
7. Writes:
   - vault, `Exposure(asset, dir) = side` (instance), nonce, wallet, next id
   - `Covered(perpdex, position_id) = id` (persistent)
   - the `Protection` with `t_active = now + activation_delay`, `t_expire = now + duration`, `window = window_for(duration, max_payout, params)`, `status = Active`, `settle_price = payout = 0`
8. Payments:
   - premium: trader → vault
   - `to_pdx = fee × share_pdx_bps / 10000`: trader → owner, if it's > 0
   - `fee − to_pdx`: trader → treasury, if it's > 0
9. Publish `Minted` as today.

`window_for` (spec §5.1: `clamp(5 min, 0.53·√(D in minutes) min, 30 min)`, rearranged to integer seconds as `√(16.853 · D)`):

```rust
if max_payout > p.big_payout { return MAX_WINDOW; }
(duration * 16_853 / 1_000).isqrt().clamp(MIN_WINDOW, MAX_WINDOW)
```

Other changes:
- Delete `release`, the `Released` event, and `AlreadyReleased`.
- `withdraw_request` and `withdraw_execute`: `cfg.owner.require_auth()` instead of admin; `withdraw_execute` transfers to `cfg.owner`.
- Add private helpers `flag(env, key) -> bool` (instance get, default `false`), `exposure`/`set_exposure` (instance, default 0) and `share(amount, bps)`.

Gotcha: `position::get` uses `try_get_position`, so an unknown position id becomes `None` instead of crashing the whole call. Don't switch it to `get_position`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p tosbi-protection`
Expected: `30 passed`. You may see unused-import warnings in `test.rs` for `Event`/`Events`; Task 2 uses them.

- [ ] **Step 5: Lint, build, commit**

```sh
cargo fmt && cargo clippy -p tosbi-protection -p tosbi-mock-perpdex -p tosbi-mock-oracle && stellar contract build
git add contracts Cargo.lock && git commit -m "feat(contract): position binding, risk caps, owner withdrawals, mock PerpDEX and oracle"
```

What the tests prove, for review:

| Test | Proves |
|---|---|
| `h1_deposit_buy`, `h4`, `e1`–`e7`, `x1`–`x7`, `x9_x10`, `c1`–`c3`, `s1_s2`, `s4_s5`, `s8` | Everything Instaward 1 promised still holds with the new quote shape |
| `h5_js_signed_quote_is_accepted` | A quote signed by the JS pricer (fixed vector) is accepted; Task 5 makes JS produce it |
| `b1` | Wrong owner, wrong direction or asset, too small, closed or unknown position → `PositionMismatch` |
| `b2` | Mark 0.5% away is accepted; 0.50001% → `MarkMoved` |
| `b3` | Position 59 s old → `PositionTooNew`; 60 s → ok |
| `b4` | Second cover on one position → `AlreadyCovered`; disabled asset; non-allowlisted PerpDEX |
| `k1` | Utilization cap at exactly 90% |
| `k2` | 25% per side and 40% per asset, including premium growth; exposure views |
| `k4` | Fee split 30/70 between owner and treasury |

---

### Task 2: Oracle reads and settlement at expiry

**Goal:** `settle(id)`, callable by anyone after expiry, pays `min(qty × loss, max_payout)` using the oracle's average price over the window ending at expiry. If the oracle is down for 2 hours, it settles on the last price instead.

**Files:**
- Create: `contracts/protection/src/oracle.rs`
- Modify: `contracts/protection/src/lib.rs`
- Apply (tests): `contracts/protection/src/test/settle.rs`; add `mod settle;` back to `test.rs`

**Interfaces:**

```rust
// oracle.rs: SEP-40 types (must match Reflector and mock-oracle)
#[contracttype] pub enum Asset { Stellar(Address), Other(Symbol) }
#[contracttype] pub struct PriceData { pub price: i128, pub timestamp: u64 }
#[allow(dead_code)]
#[contractclient(name = "OracleClient")]
pub trait Sep40 {
    fn decimals(env: Env) -> u32;
    fn resolution(env: Env) -> u32;
    fn price(env: Env, asset: Asset, timestamp: u64) -> Option<PriceData>;
    fn lastprice(env: Env, asset: Asset) -> Option<PriceData>;
}
pub fn to_7(price: i128, decimals: u32) -> i128
pub fn twap(env: &Env, oracle: &Address, p: &Params, asset: &Symbol, end: u64, window: u64) -> Option<i128>
pub fn price_at(env: &Env, oracle: &Address, asset: &Symbol, ts: u64) -> Option<i128>   // Task 3
pub fn last(env: &Env, oracle: &Address, asset: &Symbol) -> Option<i128>

// lib.rs
#[contractevent]
pub struct Settled { #[topic] pub id: u64, pub price: i128, pub payout: i128, pub early: bool, pub degraded: bool }
pub fn settle(env: Env, id: u64) -> Result<i128, Error>    // returns the payout
fn finish(env: &Env, cfg: &Config, p: &mut Protection, price: i128, early: bool, degraded: bool) -> Result<i128, Error>
```

Facts about Reflector testnet, checked on 2026-10-16 (contract `CCYOZJCOPG34LLQQ7N24YXBM7LL62R7ONMZ3G6WZAAYPB5OYKOMJRN63`):
- `decimals = 14`, `resolution = 300`, base currency USD.
- Assets are `Other("XLM")`, `Other("BTC")`, `Other("ETH")`.
- Timestamps are in seconds. `price(asset, ts)` returns the record at the 5-minute boundary at or before `ts`.
- History reaches back about 18–23 hours.

- [ ] **Step 1: Copy in the tests**

```sh
git apply --include='contracts/protection/src/test/settle.rs' docs/superpowers/plans/2026-10-16-sow1-answer-key.patch
```

Add `mod settle;` back to the `mod` lines in `test.rs`. `settle.rs` adds two helpers on the test harness: `feed_window` and `settle_at`.

- [ ] **Step 2: Run, expect a compile failure**

Run: `cargo test -p tosbi-protection st1`
Expected: `no method named settle` / `cannot find value Settled`.

- [ ] **Step 3: Implement `oracle.rs`**

- `to_7`: divide by `10^(decimals − 7)` if `decimals ≥ 7`, otherwise multiply.
- `twap`:
  1. `res = resolution()` and `last = end / res * res` (the record at or before expiry). The number of records in the window is `n = max(1, ceil(window / res))`.
  2. A record "exists at ts" only if `price(asset, ts)` returns one whose `timestamp == ts`. A different timestamp means the oracle skipped that slot.
  3. If no record exists at `last`, return `None`. It isn't published yet, or it's too old.
  4. `prev` starts as the record at `last − n·res`, the one just before the window. It may be missing.
  5. For `k` from `n−1` down to `0` (oldest first), take the record at `last − k·res`:
     - skip it if it's missing;
     - skip it if `prev` exists and `|px − prev| × 10000 > prev × record_cap_bps` (a spike);
     - otherwise add it to the sum and set `prev = px`.
  6. Return `None` if nothing was accepted, else `sum / count`.
- `price_at` and `last`: call `price(asset, ts)` / `lastprice(asset)` with `Asset::Other(symbol)` and pass the result through `to_7`. No exact-timestamp filter here.
- Add `#[allow(dead_code)]` on the `Sep40` trait. Clippy warns because nothing implements it.

- [ ] **Step 4: Implement `settle` and `finish` in `lib.rs`**

Add `mod oracle;`.

`settle`:
1. `active(env, id)`: load the protection; missing → `NotFound`; status `Settled` → `AlreadySettled`.
2. `now < t_expire` → `NotExpired`.
3. Call `oracle::twap(.., asset, t_expire, window)`:
   - `Some(px)` → settle at `px`, `degraded = false`.
   - `None` while `now < t_expire + stale_max` → return `OracleStale`. The keeper retries.
   - `None` after that → settle at `oracle::last(..)` (or `OracleStale` if there is none), `degraded = true`.
4. Call `finish(.., early = false, degraded)`.

`finish`:
1. `payout = payout_for(p, price)`. For Long, `gap = p_start − price`; for Short, `gap = price − p_start`. If `gap ≤ 0` the payout is 0, otherwise `min(qty × gap / UNIT, max_payout)`.
2. `capital −= payout`; reduce `reserved`, `Exposure(asset, dir)` and `WalletReserved(trader)` by `max_payout`.
3. Remove `Covered(perpdex, position_id)`, so the position can be covered again.
4. Set `status = Settled`, `settle_price = price`, `payout = payout`, and save with `set_persistent`.
5. If `payout > 0`, `pay(vault → trader)`. A failure returns `PaymentFailed` and rolls everything back. SOW 2 replaces this with "owed + claim".
6. Publish `Settled { id, price, payout, early, degraded }` and return `Ok(payout)`.

Don't call `require_auth` anywhere in `settle` (test `s3`).

- [ ] **Step 5: Run the tests**

Run: `cargo test -p tosbi-protection`
Expected: `44 passed`.

- [ ] **Step 6: Lint and commit**

```sh
cargo fmt && cargo clippy --tests && git add contracts && git commit -m "feat(contract): settle on the oracle TWAP at expiry"
```

| Test | Proves |
|---|---|
| `st1`, `st2` | Long and short payouts at three prices: no loss, partial loss, capped |
| `st3` | A −41% single record is ignored; a −5.9% record is averaged in |
| `st4` | No record at expiry → `OracleStale` until 2 h, then a degraded settlement on the last price, with event `degraded: true` |
| `st5` | `window_for` for every duration bucket and the size override |
| `st6` | A keeper 3 h late still uses the window at expiry, not today's price |
| `st7`, `c4`, `k3` | Settlement frees the reserve, the exposure and the position for new cover |
| `st8` | A skipped record inside the window doesn't block settlement (Review Focus 1) |
| `st9` | BTC prices: 0.01 BTC, $100,000 → $91,234.5678901 pays exactly 87.654321 USDC (Review Focus 2) |
| `x8`, `s3` | Error cases; no signature needed |
| `c5` | 20 random sequences of deposits, buys, settlements and withdrawals keep full backing and exposure = reserved |

---

### Task 3: Early settlement on close, liquidation, or reduction

**Goal:** If the position is closed, liquidated or reduced before expiry, anyone can call `settle_early(id)`. The contract reads the position itself, so it doesn't trust whoever calls.

**Files:** Modify `contracts/protection/src/lib.rs`. Apply `contracts/protection/src/test/early.rs` and add `mod early;` to `test.rs`.

**Interface:** `pub fn settle_early(env: Env, id: u64) -> Result<i128, Error>`

- [ ] **Step 1: Copy in the tests and add the `mod` line**

```sh
git apply --include='contracts/protection/src/test/early.rs' docs/superpowers/plans/2026-10-16-sow1-answer-key.patch
```

- [ ] **Step 2: Run, expect `no method named settle_early`**

Run: `cargo test -p tosbi-protection b5`

- [ ] **Step 3: Implement**, in this order:

1. `active(id)` (same errors as `settle`).
2. `position::get(perpdex, position_id)` returns `None` → `NotClosed`. A broken PerpDEX can't block expiry settlement.
3. `reduced = status == Open && size < qty`. If it's Open and not reduced → `NotClosed`.
4. `ts = now` if reduced, else `close_ts.unwrap_or(now)`.
5. `ts ≥ t_expire` → `return Self::settle(env, id)`. The protection lasted its full term.
6. `ts < t_active` → `finish` at `p_start`, which pays 0, with `early = true`. Spec decision: closing during the 5-minute delay gets no refund.
7. `reference = oracle::price_at(ts)`, or else `oracle::last()`, or else `OracleStale`.
8. Use the close price if there is one, the position wasn't merely reduced, and `|close − reference| × 10000 ≤ reference × close_tol_bps`. Otherwise use `reference`.
9. `finish(.., early = true, degraded = false)`.

- [ ] **Step 4: Run all contract tests**

Run: `cargo test -p tosbi-protection`
Expected: `50 passed`.

- [ ] **Step 5: Commit**

`git add contracts && git commit -m "feat(contract): settle early on close, liquidation or reduction"`

| Test | Proves |
|---|---|
| `b5` | Close 0.55% from the oracle → the close price is used (pays 100); settling again → `AlreadySettled` |
| `b6` | Close 6% from the oracle → the oracle price is used |
| `b7` | Close during the activation delay pays 0 even in a crash |
| `b8` | Liquidation settles like a close |
| `b9` | Reducing the position settles the whole contract at the oracle price |
| `b10` | Close after expiry → settles on the expiry TWAP, ignoring the close price |

---

### Task 4: Governance: roles, timelock, pause, vol hash

**Goal:** The PerpDEX and TOSBI jointly control parameters through a 24-hour timelock, which is 300 s on testnet. Either of them can pause sales alone.

**Files:** Create `contracts/protection/src/governance.rs`. Modify `lib.rs`: add `mod governance;` and `pub use governance::{Change, PendingChange};`. Apply `test/governance.rs` and add `mod governance;` to `test.rs`.

**Interfaces:**

```rust
#[contracttype]
pub enum Change {
    Params(Params), Pricer(BytesN<32>), Oracle(Address), Treasury(Address),
    Asset(Symbol, bool), Perpdex(Address, bool), Upgrade(BytesN<32>),
}
#[contracttype] pub struct PendingChange { pub change: Change, pub eta: u64 }
// events: ChangeProposed { #[topic] id, eta }, ChangeExecuted { #[topic] id },
//         ChangeCancelled { #[topic] id }, PauseSet { paused }, VolHashSet { hash }

pub fn propose(env: Env, change: Change) -> u64                  // admin; eta = now + params.timelock
pub fn execute(env: Env, id: u64) -> Result<(), Error>            // admin; NoChange / Timelocked
pub fn cancel(env: Env, id: u64) -> Result<(), Error>             // admin; NoChange
pub fn pause(env: Env, caller: Address) -> Result<(), Error>      // caller must be owner or operator, else Unauthorized
pub fn unpause(env: Env)                                          // admin
pub fn set_vol_hash(env: Env, hash: BytesN<32>)                   // operator
pub fn pending_change(env: Env, id: u64) -> Option<PendingChange>
pub fn vol_hash(env: Env) -> Option<BytesN<32>>
```

- [ ] **Step 1: Copy in the tests and add the `mod` line**

```sh
git apply --include='contracts/protection/src/test/governance.rs' docs/superpowers/plans/2026-10-16-sow1-answer-key.patch
```

- [ ] **Step 2: Run, expect `cannot find type Change`**

Run: `cargo test -p tosbi-protection g1`

- [ ] **Step 3: Implement**

Write a second `#[contractimpl] impl ProtectionContract { ... }` block in `governance.rs`.

Gotcha: the macro generates code that names the client and args types, so import them, or you'll get confusing errors:
`use crate::{config, flag, set_persistent, Error, Key, Params, ProtectionContract, ProtectionContractArgs, ProtectionContractClient};`

Rules:
- `propose`:
  - id comes from `Key::NextChange` (instance, default 0, then incremented);
  - store `PendingChange` under `Key::Change(id)` with `set_persistent`;
  - publish `ChangeProposed`.
- `execute`:
  - missing → `NoChange`; `now < eta` → `Timelocked`;
  - otherwise remove the entry and apply the change. `Params`, `Pricer`, `Oracle` and `Treasury` edit the `Config`. `Asset` and `Perpdex` set instance flags. `Upgrade` calls `env.deployer().update_current_contract(ContractExecutable::Wasm(hash))`; don't use the deprecated `update_current_contract_wasm`;
  - save the config and publish `ChangeExecuted`.
- `pause` / `unpause`: store `Key::Paused`. Publish `PauseSet` only when the value actually changes.
- `set_vol_hash`: `operator.require_auth()`, store it under `Key::VolHash`, publish `VolHashSet`.

Don't touch settlement or withdrawals for pause. `check_quote` already returns `Paused`.

- [ ] **Step 4: Run everything, lint, build**

```sh
cargo test                        # expect: 58 passed
cargo fmt && cargo clippy --tests # expect: no warnings
stellar contract build            # three .wasm files in target/wasm32v1-none/release/
```

- [ ] **Step 5: Commit**

`git add contracts && git commit -m "feat(contract): timelocked governance, pause, vol hash"`

| Test | Proves |
|---|---|
| `g1` | A change waits exactly `timelock`; it executes once |
| `g2` | Cancel works; non-admins can't propose |
| `g3` | Owner or operator can pause; strangers can't; paused still settles and withdraws; admin unpauses |
| `g4` | Pricer key rotation: the old key's signatures fail, the new one's pass |
| `g5` | Enabling ETH and disabling XLM |
| `g6` | Only the operator posts the vol hash |
| `b11` | A delisted PerpDEX's live protections still settle early (allowlist gates new sales only) |
| `g7` | Disabling an asset still settles live cover (Review Focus 3) |

---

### Task 5: Pricer quotes for positions

**Goal:** `/api/quote` prices a specific position. It takes asset, direction, mark price and owner from the PerpDEX, concentration from on-chain exposure, and refuses cover past the liquidity cap or during a vol shock.

**Files:**
- Modify: `pricer/src/quote.ts`, `pricer/src/chain.ts`
- Rewrite: `pricer/src/service.ts`
- Apply (tests): `pricer/test/quote.test.ts`, `pricer/test/service.test.ts`

**Interfaces:**

```ts
// quote.ts: Quote gains, right after `vault`:
perpdex: string;
/** 32 bytes as 64 hex characters. */
position_id: string;
// TYPES gains: perpdex: "address", position_id: "bytes"
// quoteToScVal: for "position_id" pass Buffer.from(q.position_id, "hex")

// chain.ts
export const ERRORS: string[]            // index = contract error code; "" at 0, then the 28 names from Task 1 in order
export async function invokeAs(n: Net, secret: string, contract: string, method: string, args: xdr.ScVal[]): Promise<Receipt>
// prepareInvoke as the secret's public key → sign → submit

// service.ts
export const ASSETS = ["XLM", "BTC", "ETH"] as const;
export interface Position { owner: string; asset: string; direction: number; size: bigint; mark_price: bigint; status: number }
export interface VaultState { capital: bigint; reserved: bigint; locked: bigint }
export interface Shock { factor: number; until: number }
export interface Deps {
  secret: string; vault: string; perpdex: string;
  volTables: Record<string, VolTable>;
  now: () => bigint; nonce: () => bigint;
  position: (id: string) => Promise<Position>;
  vaultState: () => Promise<VaultState>;
  exposure: (asset: string, dir: Direction) => Promise<bigint>;
  depth: (asset: string) => Promise<number | null>;   // USDC to move the asset 25%; null = not estimated
  shock: (asset: string) => Promise<Shock | null>;
}
export async function makeQuote(input: Record<string, unknown>, d: Deps): Promise<SignedQuote>
// input: { positionId: 64 lowercase hex, amount: decimal string, duration: integer secs, rangeBps: 500..5000 }
// SignedQuote.breakdown gains `concentration`
```

- [ ] **Step 1: Copy in the tests**

```sh
git apply --include='pricer/test/quote.test.ts' --include='pricer/test/service.test.ts' docs/superpowers/plans/2026-10-16-sow1-answer-key.patch
```

- [ ] **Step 2: Run, expect failures**

Run: `cd pricer && node --test test/quote.test.ts test/service.test.ts`
Expected: the H5 vector mismatches, and service tests fail on the missing `positionId` handling.

- [ ] **Step 3: Implement `quote.ts` and `chain.ts`** using the interface above. The H5 test vector in `quote.test.ts` is the same one Task 1's Rust test `h5` checks. When both pass, JS and Rust agree byte for byte.

- [ ] **Step 4: Rewrite `makeQuote`**, in this order:

1. Validate the inputs. Any problem is a `BadRequest`:
   - `positionId` must match `/^[0-9a-f]{64}$/`
   - `duration` must be an integer
   - `rangeBps` must be an integer from 500 to 5000
   - `amount` goes through `parseUnits` and must be > 0 and ≤ 50,000
2. Fetch the position and the vault state together. Any failure → `Upstream`.
3. Refuse with `BadRequest` if:
   - `status ≠ 0`
   - there's no vol table bucket for `(asset, duration)`
   - `amount × UNIT > size × mark_price` (more cover than the position is worth)
4. If `mark_price ≤ 0` → `Upstream`.
5. `dir = direction === 0 ? "long" : "short"`. Fetch the exposure for this side and the opposite side, the depth and the shock. Any failure → `Upstream`.
6. A shock is active while `shock.until > now`. Then:
   - durations ≤ 3600 → `Upstream("... paused ...")`
   - longer durations → σ is multiplied by `shock.factor`
7. Liquidity cap: if `depth !== null` and `(side + other + maxPayout) / 1e7 > 0.5 × depth` → `BadRequest("<asset> liquidity cap reached")`.
8. `available = capital − locked`.
   - `utilization = (reserved + maxPayout) / available`
   - `concentration = (side + maxPayout) / available`
   - Both are `Infinity` if `available ≤ 0`.
   - The old code divided by `capital`. That was a bug.
9. Build the quote as before:
   - `trader = pos.owner`, `asset = pos.asset`, `p_start = pos.mark_price`
   - `perpdex = d.perpdex`, `position_id = positionId`

- [ ] **Step 5: Run the pricer tests**

Run: `node --test test/quote.test.ts test/service.test.ts test/pricing.test.ts`
Expected: all pass.

`npm run typecheck` still fails in `worker.ts`; Task 8 fixes that.

- [ ] **Step 6: Commit**

`git add pricer && git commit -m "feat(pricer): quote a specific position with exposure, liquidity cap and vol shock"`

---

### Task 6: Multi-asset calibration and published vol tables

**Goal:** Vol tables for XLM, BTC and ETH, calibrated from Coinbase (Binance blocks Cloudflare and US servers), refreshed every 4 hours. Their hash is recorded on-chain so partners can audit quotes.

**Files:**
- Move: `pricer/vol-table.json` → `pricer/vol-tables/XLM.json` (use `git mv`)
- Rewrite: `pricer/scripts/calibrate.ts`
- Create: `pricer/vol-tables/BTC.json`, `pricer/vol-tables/ETH.json` (generated), `pricer/scripts/publish-vol.ts`, `.github/workflows/recalibrate.yml`
- Modify: `.gitignore` (add `pricer/vol-bundle.json`)

**Interfaces:**
- `npm run calibrate -- <XLM|BTC|ETH>` writes `vol-tables/<ASSET>.json`. The JSON shape is unchanged, plus `"asset"`.
- `node scripts/publish-vol.ts [--post]` writes `vol-bundle.json` (`JSON.stringify({XLM, BTC, ETH})`) and prints its sha256 in hex. With `--post` it also calls `set_vol_hash` using `OPERATOR_SECRET`, `RPC_URL`, `NETWORK_PASSPHRASE` and `VAULT_ID` from the environment.

- [ ] **Step 1: Rewrite `calibrate.ts`**

Keep the calibration maths (`expectedLoss`, `impliedSigma`, floor and cap). Change only:
- **Asset argument:** `process.argv[2]`, with floors `{ XLM: 0.6, BTC: 0.5, ETH: 0.5 }`. Throw a usage error for anything else.
- **Data source:**
  - `GET https://api.exchange.coinbase.com/products/<ASSET>-USD/candles?granularity=<300|3600>&start=<ISO>&end=<ISO>` with a `user-agent` header.
  - At most 300 candles per request. Rows come back **newest first** as `[time, low, high, open, close, volume]`.
  - Page through 30 days of 5-minute candles and 90 days of 1-hour candles.
  - Collect closes into a `Map` keyed by time, then sort ascending.
- **Output path:** `vol-tables/<ASSET>.json`. Update the `source` string.

- [ ] **Step 2: Generate the tables and check them**

```sh
cd pricer && mkdir -p vol-tables && git mv vol-table.json vol-tables/XLM.json
for a in XLM BTC ETH; do npm run calibrate -- $a; done
```

Expected for each asset: a 7-row table. Every `down`/`up` is ≥ the floor (0.6 for XLM, 0.5 for BTC/ETH) and ≤ 3. `samples` is about 8,600 for the short buckets and about 2,100 for 1d.

- [ ] **Step 3: Write `publish-vol.ts`** to match the interface above, and run it.

Run: `node scripts/publish-vol.ts`
Expected: a 64-hex-character hash, and `vol-bundle.json` is created. Add `pricer/vol-bundle.json` to `.gitignore`.

- [ ] **Step 4: Write the workflow.** `.github/workflows/recalibrate.yml` should:
  - run on `cron: "0 */4 * * *"` and on `workflow_dispatch`
  - use Node 24 with working directory `pricer`
  - run `npm ci`, calibrate all three assets, then `node scripts/publish-vol.ts`
  - upload with `npx wrangler kv key put vol-tables --path vol-bundle.json --binding KV --remote`
  - finish with `node scripts/publish-vol.ts --post`
  - read env from repo variable `VAULT_ID` and secrets `OPERATOR_SECRET`, `CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ACCOUNT_ID`

  Upload **before** posting the hash, so the on-chain hash never points at tables the Worker doesn't have yet.

- [ ] **Step 5: Commit**

`git add -A pricer .github .gitignore && git commit -m "feat(pricer): Coinbase calibration for XLM/BTC/ETH, published vol tables"`

---

### Task 7: Keeper and market data

**Goal:** A keeper loop that settles whatever is due. Market-data helpers for the vol-shock pause and the liquidity cap.

**Files:** Create `pricer/src/keeper.ts` and `pricer/src/market.ts`. Apply `pricer/test/keeper.test.ts` and `pricer/test/market.test.ts`.

**Interfaces:**

```ts
// keeper.ts
export interface KeptProtection { status: number; t_expire: bigint; perpdex: string; position_id: Uint8Array; qty: bigint }
export interface KeeperDeps {
  now: () => number;
  nextId: () => Promise<number>;
  cursor: { get: () => Promise<number>; set: (id: number) => Promise<void> };  // first id that may still be active
  protection: (id: number) => Promise<KeptProtection | undefined>;
  position: (perpdex: string, id: Uint8Array) => Promise<{ status: number; size: bigint } | undefined>;
  call: (method: "settle" | "settle_early", id: number) => Promise<string>;
  log: (msg: string) => void;
}
export async function runKeeper(d: KeeperDeps): Promise<void>

// market.ts
export function realizedVol(closes: number[], stepSecs: number): number   // annualized, sample stdev of log returns
export function bookDepth(levels: [number, number][], move: number, side: "bids" | "asks"): number
export async function recentCloses(asset: string, stepSecs: number, count: number): Promise<number[]>  // oldest first
export async function depthUsd(asset: string): Promise<number>   // Coinbase + Kraken, thinner side, 25% move
```

- [ ] **Step 1: Copy in the tests and run them** (they fail: module not found)

```sh
git apply --include='pricer/test/keeper.test.ts' --include='pricer/test/market.test.ts' docs/superpowers/plans/2026-10-16-sow1-answer-key.patch
cd pricer && node --test test/keeper.test.ts test/market.test.ts
```

- [ ] **Step 2: Implement `runKeeper`**

1. Read the cursor and `nextId` together.
2. For each id from the cursor up to `nextId`:
   - **Missing or already settled** (`status ≠ 0`): skip it. While every id so far has been settled, also move the cursor past it.
   - **Expired** (`now ≥ t_expire`): call `settle`.
   - **Not expired:** read the position. If it's closed, or `size < qty`, call `settle_early`.
3. Wrap each id's work in `try/catch`. Log `#<id> failed: <message>` and keep going (Review Focus 4).
4. Write the cursor **only if it changed**. Workers KV allows 1,000 writes a day on the free plan, and this runs 1,440 times a day.

- [ ] **Step 3: Implement `market.ts`**

- `realizedVol`: log returns `ln(c[i+1] / c[i])` → sample variance (`n − 1`) → `sqrt(variance × YEAR_SECS / stepSecs)`.
- `bookDepth`:
  - `limit = best × (1 − move)` for bids, or `best × (1 + move)` for asks;
  - sum `price × size` until a level falls outside the limit.
- `recentCloses`:
  - Coinbase `candles?granularity=<step>` (no start/end) returns the latest rows, newest first;
  - take `count + 1` of them, reverse, and map to the close.
- `depthUsd`:
  - fetch Coinbase `book?level=2` and Kraken `Depth?pair=<XLMUSD|XBTUSD|ETHUSD>&count=500` (the result is the first value of `result`);
  - add up the bids from both venues and the asks from both venues, then return the smaller total.

Coinbase and Kraken both answer from Cloudflare. Binance does not.

- [ ] **Step 4: Run the tests**

Run: `node --test test/keeper.test.ts test/market.test.ts`
Expected: 6 pass.

- [ ] **Step 5: Commit**

`git add pricer && git commit -m "feat(pricer): keeper loop, realized vol and order-book depth"`

---

### Task 8: Worker API and cron

**Goal:** Wire everything into the Worker: position routes, new quote input, settle actions, a vol-tables endpoint, and a `scheduled` handler for the keeper, vol-shock watch and daily depth.

**Files:** Rewrite `pricer/src/worker.ts` and `pricer/wrangler.jsonc`.

**Interfaces:**

```ts
export interface KV { get(key: string): Promise<string | null>; put(key: string, value: string, opts?: { expirationTtl?: number }): Promise<void> }
// Env adds: PERPDEX_ID, KEEPER_SECRET, KV
export async function keep(env: Env): Promise<void>
export default { fetch(req, env), scheduled(event: { cron: string }, env, ctx: { waitUntil(p: Promise<unknown>): void }) }
```

**Routes:**

| Route | Behaviour |
|---|---|
| `GET /api/state?addr=` | as before, plus `paused`, `nextId`, `perpdex`, and `exposure: { XLM: { long, short }, BTC: {...}, ETH: {...} }` |
| `GET /api/protection?id=` | unchanged |
| `GET /api/position?id=<hex>` | PerpDEX `get_position` |
| `GET /api/vol-tables` | the raw KV string `vol-tables`, else `JSON.stringify` of the bundled tables |
| `POST /api/quote` | `{ positionId, amount, duration, rangeBps }` → `makeQuote` with real deps (KV keys `depth:<A>` and `shock:<A>`) |
| `POST /api/tx` | actions `trust`, `deposit`, `open {asset, direction, notional}`, `close {positionId, price?}`, `liquidate {positionId}`, `reduce {positionId}` (halves), `settle {id}`, `settle_early {id}`, `withdraw_request {amount}`, `withdraw_execute`, `pause` |
| `POST /api/submit`, `POST /api/faucet` | unchanged |

**Encoding notes:**
- Enums such as `Direction` go over the wire as `u32`.
- The oracle asset is `scvVec([scvSymbol("Other"), scvSymbol(code)])`.
- `Option<i128>::None` is `xdr.ScVal.scvVoid()`.
- A position id is `nativeToScVal(Buffer.from(hex, "hex"))`.
- `open` converts USD notional to size with `notional × UNIT / oraclePrice`, where `oraclePrice` is `lastprice` from `config().oracle`, rescaled from 14 to 7 decimals.

**Cron (`scheduled`):**
- `"0 0 * * *"` refreshes depth for all three assets into KV.
- Anything else runs `keep(env)` and the vol-shock watch together.
  - **Vol-shock watch:** realized vol of the last 60 one-minute closes vs `max(down, up)` of the `"3600"` bucket. If the realized vol is above twice that, `put("shock:<A>", {factor: realized/σ, until: now+900}, {expirationTtl: 900})`.
- Catch and log every error, so one failure doesn't stop the rest.

**`wrangler.jsonc`:**
- `"triggers": { "crons": ["* * * * *", "0 0 * * *"] }`
- `"kv_namespaces": [{ "binding": "KV", "id": "REPLACE_WITH_KV_NAMESPACE_ID" }]` (filled in Task 11)
- `PERPDEX_ID` in `vars`

- [ ] **Step 1: Implement** the routes and cron.
- [ ] **Step 2: Typecheck and bundle**

```sh
cd pricer && npm run typecheck && npm test && npx wrangler deploy --dry-run --outdir /tmp/tosbi-dist
```

Expected: no type errors, 25 tests pass, and wrangler lists the bindings and prints `--dry-run: exiting now.`

- [ ] **Step 3: Commit**

`git add pricer && git commit -m "feat(worker): position, settlement and governance routes; keeper and vol-shock cron"`

---

### Task 9: Deploy, admin and an end-to-end run on a local network

**Goal:** One command deploys everything with real roles: a 2-of-2 admin and separate owner, operator and keeper keys. One script runs the whole product through the Worker against that deployment.

**Files:**
- Rewrite: `scripts/deploy.sh`, `pricer/scripts/e2e.ts`
- Create: `scripts/admin.sh` (`chmod +x`)

**`deploy.sh` must:**
1. Create the keys `tosbi-{owner,operator,keeper,issuer,treasury,pricer,signer-tosbi,signer-pdx}`, plus a **fresh** `tosbi-admin-<unix time>`. Once an admin account is multisig its own key can't sign, so each deploy needs a new one. Fund them all.
2. Make the admin 2-of-2:
   - `stellar tx new set-options --source <admin> --signer <addr> --signer-weight 1`, once for each signer;
   - then `--master-weight 0 --low-threshold 2 --med-threshold 2 --high-threshold 2`.
3. Add tUSDC trustlines for owner and treasury, deploy the SAC, and build.
4. Deploy the oracle:
   - **local:** deploy `tosbi_mock_oracle` and `set` XLM `20000000000000`, BTC `10000000000000000000`, ETH `400000000000000000` at the current time;
   - **testnet:** use Reflector `CCYOZJCOPG34LLQQ7N24YXBM7LL62R7ONMZ3G6WZAAYPB5OYKOMJRN63`.
5. Deploy `tosbi_mock_perpdex -- --oracle <oracle>`. Deploy `tosbi_protection` with `--config <json> --assets '["XLM","BTC","ETH"]' --perpdexes '["<perpdex>"]'`.
   - In the JSON, i128 values are strings (`"200000000"`); u32 and u64 values are numbers.
   - `min_position_age` and `activation_delay` are `0` on local and `60` / `300` on testnet. `timelock` is `300` on both.
6. The issuer pays 10,000 tUSDC to the owner, and the owner calls `deposit_capital`.
7. Write `pricer/.dev.vars` with: `RPC_URL`, `NETWORK_PASSPHRASE`, `VAULT_ID`, `PERPDEX_ID`, `ORACLE_ID`, `ADMIN_ID`, `ASSET_CODE`, `ASSET_ISSUER`, `PRICER_SECRET`, `FAUCET_SECRET`, `KEEPER_SECRET`, `OPERATOR_SECRET`.

**`admin.sh <net> <fn> [args...]`:**
- Read `VAULT_ID` and `ADMIN_ID` from `.dev.vars`.
- Pipe `stellar contract invoke --source-account $ADMIN_ID --build-only -- <fn> <args>` → `stellar tx simulate` → `stellar tx sign --sign-with-key tosbi-signer-tosbi` → `stellar tx sign --sign-with-key tosbi-signer-pdx` → `stellar tx send`.

**`e2e.ts`:**
- Same style as today. Pass an in-memory `KV` (a `Map`) inside `env`.
- Steps, in order:
  1. trustline and faucet
  2. open four positions: long $1,000, long $2,000, long $60,000, short $300
  3. if `min_position_age > 0`, wait it out
  4. buy cover on the first position
  5. covering it again → `AlreadyCovered`
  6. 400 on the second position → `WalletCapExceeded`
  7. 50,000 at 50% on the big position → `InsufficientCapital`
  8. 1,001 on the $1,000 position → HTTP 400
  9. buy short cover
  10. withdraw as the trader → `needs` = [owner]
  11. pause as the trader → `Unauthorized`
  12. `settle_early` on an open position → `NotClosed`
  13. close the first position, call `worker.scheduled({cron:"* * * * *"}, env, ctx)` and await every `waitUntil` promise. The first protection must now be settled
  14. deposit
  15. malformed requests → 400

- [ ] **Step 1: Write the three scripts.** The answer key has working versions; write yours first.
- [ ] **Step 2: Deploy locally and run end to end**

```sh
export DOCKER_HOST=$(docker context inspect --format '{{.Endpoints.docker.Host}}')
stellar container start local --protocol-version 29        # wait until getHealth says healthy
scripts/deploy.sh local
cd pricer && node scripts/e2e.ts
```

Expected: 15 `ok` lines, a `keeper #0 settle_early: SUCCESS` log line, then `E2E passed`.

- [ ] **Step 3: Prove the admin really is 2-of-2**

```sh
T=$(stellar keys address tosbi-treasury)
scripts/admin.sh local propose --change "{\"Treasury\":\"$T\"}"      # succeeds
V=$(grep '^VAULT_ID=' pricer/.dev.vars | cut -d= -f2)
stellar contract invoke --id $V --source tosbi-operator --network local --send=no -- pending_change --id 0   # shows the change
scripts/admin.sh local cancel --id 0                                  # succeeds
A=$(grep '^ADMIN_ID=' pricer/.dev.vars | cut -d= -f2)
stellar contract invoke --id $V --source-account $A --network local --build-only -- unpause \
  | stellar tx simulate --source-account $A --network local \
  | stellar tx sign --sign-with-key tosbi-signer-tosbi --network local \
  | stellar tx send --network local                                   # one signer only
```

Expected: the last command fails with `TxBadAuth`.

- [ ] **Step 4: Publish the vol hash locally**

```sh
cd pricer && node --env-file=.dev.vars scripts/publish-vol.ts --post
```

Expected: `set_vol_hash SUCCESS`. Note `--env-file`: `source .dev.vars` breaks because the network passphrase contains spaces.

- [ ] **Step 5: Stop the network and commit**

```sh
stellar container stop local
git add scripts pricer/scripts && git commit -m "feat(scripts): role-based deploy, 2-of-2 admin calls, full end-to-end run"
```

---

### Task 10: Demo page

**Goal:** A reviewer with no technical background can do the whole lifecycle in the browser: open a position, protect it, close, liquidate or halve it, watch it settle, and see every rejection explained.

**Files:** Rewrite the `<body>` and `<script>` of `pricer/public/index.html`. Keep the `<head>` and styles unchanged.

**Page sections, in order:**
0. **Vault:** capital, reserved, queued and free, as today. Add "Reserved by asset" (long and short for each asset), a "Sales paused" badge, and links to both contracts.
1. **Get test funds:** unchanged.
2. **Open a test position:** asset, direction and USD size → `open`. Then a table of your positions with Close, Close at a chosen price (one shared input), Halve and Liquidate.
3. **Protect a position:**
   - a select listing **your open positions only**, plus amount, duration and range → quote → Buy;
   - "Try to exceed the pool" opens a $60,000 XLM long, then quotes 50,000 at 50%.
4. **Your protections:** status is Pending, Active, or Settled at a price with its payout. Show **Settle** after expiry and **Settle early** before it.
5. **Vault owner actions:** deposit, withdraw request/execute, Pause. For a reviewer the last three show the contract refusing.

**Rules:**
- Save position ids and protection ids in `localStorage` under `tosbi.<vault>.<wallet>.<positions|ids>` (Review Focus 5).
- Add a friendly message to `MESSAGES` for every new error name.
- Use the existing `busy`, `action`, `signAndSubmit` and `log` helpers.
- Build tables with DOM methods, not `innerHTML`, because protection data comes from the chain.

- [ ] **Step 1: Write the page.**
- [ ] **Step 2: Check the script parses**

```sh
python3 -c "s=open('pricer/public/index.html').read(); i=s.rindex('<script>'); j=s.rindex('</script>'); open('/tmp/page.js','w').write(s[i+8:j])" && node --check /tmp/page.js && echo parses
```

- [ ] **Step 3: Look at it.** Run `cd pricer && npx wrangler dev`, open http://localhost:8787, and check that the vault panel loads with no console errors. Wallet actions need Freighter on testnet, so they're tested in Task 11.
- [ ] **Step 4: Commit**

`git add pricer/public && git commit -m "feat(demo): positions, settlement and new rejections"`

---

### Task 11: Testnet deployment, README, evidence

**Goal:** Everything live on testnet, the README pointing at it, and the SOW evidence collected.

- [ ] **Step 1: Deploy the contracts** with `scripts/deploy.sh testnet`. Save the output; the README needs it.
- [ ] **Step 2: Configure and deploy the Worker**

```sh
cd pricer
npx wrangler kv namespace create KV          # paste the id into wrangler.jsonc
# copy VAULT_ID, PERPDEX_ID and ASSET_ISSUER from the deploy output into wrangler.jsonc "vars"
for s in PRICER_SECRET FAUCET_SECRET KEEPER_SECRET; do grep "^$s=" .dev.vars | cut -d= -f2 | npx wrangler secret put $s; done
npx wrangler deploy
```

- [ ] **Step 3: Publish vol tables.** On GitHub, add repo variable `VAULT_ID` and secrets `OPERATOR_SECRET` (from `.dev.vars`), `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID`. Run the "Recalibrate vol tables" workflow by hand once. It should pass, and `curl https://<worker>/api/vol-tables | shasum -a 256` should equal the vault's `vol_hash`.
- [ ] **Step 4: Run end to end on testnet** with `cd pricer && node scripts/e2e.ts`. It waits about 70 s for the minimum position age.
- [ ] **Step 5: Manual demo checklist with Freighter on testnet.** Do each of these and screenshot the result:
  - [ ] fund (XLM + tUSDC); open an XLM long; wait a minute; quote and buy
  - [ ] "Try to exceed the pool" shows the pool rejection
  - [ ] a second protection on the same position is rejected
  - [ ] 6+ minutes after buying, "Close at a chosen price" 10% below the market → settles at the **oracle** price, because the close is an outlier
  - [ ] close another protected position at market → the keeper settles it within about a minute, with no button press
  - [ ] Liquidate and Halve each settle their protection
  - [ ] a 15-minute protection settles by itself shortly after expiry
  - [ ] Request withdrawal and Pause are both refused, with the explained message
  - [ ] **switch Freighter to another account and reload:** the first account's positions don't appear (Review Focus 5)
- [ ] **Step 6: Update `README.md`.**
  - Replace the "Testnet deployment" table with the new IDs: vault, PerpDEX, admin (2-of-2) and its signers, owner, operator, keeper, pricer key, and the demo URL.
  - Copy the "Run", deliverables and "Where this differs" sections from the answer key's README hunk, then check every link works.
- [ ] **Step 7: Final checks and commit**

```sh
cargo test && cargo fmt --check && cargo clippy --tests && stellar contract build
cd pricer && npm test && npm run typecheck
git add -A && git commit -m "docs: SOW 1 testnet deployment and run instructions"
```

Hand your lead:
- the demo URL
- the contract IDs and deploy transaction hashes
- the `cargo test` output (58 passed) and `npm test` output (25 pass)
- the E2E output
- the screenshots from Step 5

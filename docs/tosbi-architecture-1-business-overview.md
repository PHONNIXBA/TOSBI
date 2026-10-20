# TOSBI Protection — Business Overview
## Architecture 1: Reserve-Backed Position Protection

Draft v0.1 — for PerpDEX partners, investors, and internal stakeholders

---

## 1. The one-paragraph version

TOSBI lets a perpetual trader protect an open position against a market move in the wrong direction, in one tap, without leaving the PerpDEX. The trader pays a small fixed cost upfront. If the market moves against them during the protection period, they receive a payout that matches their actual loss, up to a cap they chose. If the market doesn't move against them, the protection simply expires. The PerpDEX supplies a pool of capital that backs every payout; TOSBI supplies the contracts, pricing, settlement, and risk controls. The pool can never promise more than it holds.

---

## 2. The problem we solve

Perpetual traders use leverage. A 10× long position loses everything on a 10% drop. Today a trader who wants to limit that risk has three bad options:

- **Stop-loss orders** — close the position on a dip and miss the recovery. Traders hate being "stopped out."
- **Options hedging** — buy a put on Deribit or Binance. Requires understanding strikes, expiries, Greeks, and moving money to another venue. Almost no retail perp trader does this.
- **Nothing** — the most common choice, and the reason liquidation cascades wipe out billions in a single day.

Bybit's Perp Protect proved there is demand for a fourth option: attach protection directly to the position, quote a price, pay, done. TOSBI brings that to on-chain PerpDEXs, starting on Stellar, where no such product exists.

---

## 3. What the trader experiences

**Buying.** Next to an open position there's a "Protect" button. The trader picks three things: how much to protect, for how long (15 minutes up to 7 days), and how deep the coverage goes (for example, the first 25% of a drop). They see one number — the total cost — and confirm.

**Example.** A trader is long $1,000 of XLM at $0.20. They protect the full $1,000 for 1 day, covering the first 25% of any drop. Cost: about $29.

**Waiting.** For 24 hours, nothing to do. Their worst case on that position is now known.

**Ending.** One of four things happens:

| What XLM does | Position result | Protection pays | Trader's net |
|---|---|---|---|
| Rises to $0.23 | +$150 | $0 (expires) | +$121 |
| Falls to $0.17 | −$150 | +$150 | −$29 |
| Crashes to $0.10 | −$500 | +$250 (the cap) | −$279 |
| Trader closes early at $0.18 | −$100 | +$100 | −$29 |

Payout lands in the trader's wallet automatically. There's no claim to file and no one to approve it.

The same applies in mirror image to a **short** position, which loses when the price rises. Short protection covers the first 25% of a *rise* — from $0.20 up to $0.25 — and pays the same way. Because rises are usually gentler than crashes, it is priced from a separate volatility figure and costs slightly less (about $25 in this example).

**What the trader never sees:** the pricing model, the oracle, the capital pool, the risk limits. From their side it is three choices, one price, one automatic outcome.

---

## 4. What the PerpDEX gets

**A new product, no build.** TOSBI provides the contracts, pricing engine, settlement, and risk monitoring. The PerpDEX integrates a "Protect" button and exposes its position data.

**Premium income.** Every premium a trader pays goes into the PerpDEX's Protection Capital pool. Over time, premiums collected from traders who didn't need the protection exceed payouts to traders who did. That spread is the PerpDEX's return on the capital it committed.

**A transaction fee.** On top of the premium, each purchase carries a small Protection Fee (initially 0.3% of the protected amount). This is TOSBI's revenue; a share can be routed to the PerpDEX.

**Stickier traders and bigger positions.** Traders who can cap their downside hold positions through volatility instead of panic-closing, and size up with more confidence. Both increase trading volume on the venue.

**Fewer catastrophic liquidations.** Protected traders receive payouts during crashes, which keeps them solvent and trading afterward.

---

## 5. How the money works

There are two pools of money and a strict rule connecting them.

**Protection Capital.** The PerpDEX deposits USDC into a vault. This is the money that pays traders. It stays the PerpDEX's property and can be withdrawn with a 7-day notice, as long as it isn't currently backing live contracts.

**Reservations.** Every time protection is sold, its maximum possible payout is set aside from the vault — "reserved" — for the life of the contract. In the example above, $250 is reserved for 24 hours. When the contract settles, the reservation is released (minus any payout).

**The rule: full backing.** The total of all reservations can never exceed the vault balance. If a new contract would break that rule, it isn't sold. This means the pool is solvent by construction — it is impossible for TOSBI to owe more than the vault holds, even if every protected position pays out its maximum on the same day.

**Premiums replenish the pool.** Premiums are paid into the vault immediately, so a busy day adds capital as well as risk.

---

## 6. How prices are set

The premium is not guessed. It is calculated from the same standard model professional options desks use, with two adjustments that protect the pool.

**Fair price.** For each asset and each duration, TOSBI maintains a measure of how much the price typically moves over that period, calibrated from the asset's actual recent history (including its crashes, not just calm days). Falls and rises are measured separately, so long and short protection are priced from different figures. From that, the fair price of "cover the first 25% of a 1-day drop" is a formula. For XLM in the example it comes to roughly 2% of the protected amount.

**Utilization adjustment.** As more of the pool is reserved, the premium rises. This keeps capacity available and pays the PerpDEX more when its capital is working harder.

**Concentration adjustment.** If many traders are protecting the same asset in the same direction — everyone long XLM and nervous — the premium for that specific protection rises. A crowded bet is a riskier bet for the pool, and the crowd pays for it.

**A safety margin** of 5% and a minimum premium round it out. In the example, a 2.1% fair price becomes roughly 2.6% after adjustments, plus the 0.3% fee, for a total of about 2.9%.

The pricing table is published so the PerpDEX can audit every quote.

---

## 7. How the pool is protected

"Full backing" guarantees solvency. These controls limit how much of the pool a single event can consume.

| Control | What it does |
|---|---|
| Concentration cap | No single asset-and-direction can reserve more than 25% of the pool; no single asset more than 40% |
| Wallet cap | No single trader can hold more than 2% of the pool in active protection |
| Utilization cap | The last 10% of the pool is never sold — it's a buffer |
| Liquidity cap | Protection sold on an asset is limited relative to how much it would cost to move that asset's price |
| Dynamic pricing | Premiums rise automatically as risk concentrates (section 6) |
| Vol-shock pause | If an asset's volatility suddenly doubles, short-duration protection on it pauses for 15 minutes while prices are refreshed |

**Worst case, by design.** In a 30% one-day crash with the XLM-long book at its 25% cap, the pool pays out at most 25% of its capital on XLM longs. Painful, but planned for and survivable — and the pool has collected premiums on every one of those contracts.

---

## 8. How we stop people gaming it

| Threat | Defense |
|---|---|
| Buy protection, then close the position and keep the payout | Protection is tied to the position. Close the position and the protection settles at that moment. |
| Buy protection the instant a crash starts | 5-minute activation delay; price quotes expire after 30 seconds. |
| Push the price down for one second to trigger a payout | Settlement uses a 20–30-minute average price across multiple sources, not a single tick. Moving the average for that long costs far more than any payout. |
| Close a position at a fake price on a thin book | If the close price is more than 1.5% from the independent oracle price, the oracle price is used instead. |
| One informed whale draining the pool | Per-wallet cap of 2% of the pool. |
| Buying protection cheaply before a known event | Surcharges around token unlocks and major scheduled announcements. |

None of this stops a trader from being right. Some will buy protection, the market will fall, and they'll be glad they did. That is the product working. The pool is paid by the majority who bought protection and didn't need it.

---

## 9. What we measure

**Loss ratio** — payouts divided by premiums — is the one number that says whether pricing is right. Target: below 80% on a rolling 30-day basis. Above 100% for two weeks triggers a price increase; persistently below 40% with low demand triggers a price decrease to improve conversion.

Also tracked: pool utilization, concentration by asset, oracle health, settlement timing, and how many traders complete a purchase after seeing a quote.

---

## 10. Why this design first

We evaluated five architectures against academic literature and live products (Bybit, Nami, Bumper, Panoptic, Derive). This one wins the first version because:

- **It cannot go insolvent.** Full backing is enforced by the contract, not by a model.
- **Prices are auditable.** Standard formulas, published inputs.
- **It's the simplest to build and audit** on Stellar's smart-contract platform.
- **Traders already understand it.** The experience matches Bybit's Perp Protect, the market's benchmark.
- **The literature says the alternatives aren't ready.** Delta-hedging short-dated crypto options performs poorly and Stellar has no deep on-chain perp venue to hedge on; streaming designs give traders an unknown total cost; binary "touch" designs are the easiest to manipulate.

**Its cost:** capital efficiency. The pool must hold 100% of the maximum payout for every live contract. The roadmap addresses this.

---

## 11. Roadmap

| Phase | What changes | Why |
|---|---|---|
| **v0.1 (this document)** | Fixed duration menu, full backing, single PerpDEX on Stellar | Prove the product and the loss ratio |
| **v0.2** | Any duration (not just the menu), partial position closes, multiple protections per position | Trader flexibility |
| **v1.1** | Richer volatility surface, second oracle for cross-checks, event calendar | Pricing accuracy |
| **v2** | Tranched vault: PerpDEX holds a first-loss slice, outside investors can supply senior capital | Scale capital beyond one PerpDEX's balance sheet |
| **v2.x** | Open-ended "streaming" protection with no expiry | Differentiated product; arbitrary duration solved properly |
| **Later** | Multi-chain; partial hedging once a deep perp venue exists on Stellar | Capital efficiency |

---

## 12. What we need from a PerpDEX partner

1. Commit an initial Protection Capital deposit (recommended minimum: enough to back roughly two weeks of expected demand; we'll size this together from the venue's open interest).
2. Expose position data on-chain: owner, asset, direction, size, mark price, and — ideally — the close price when a position closes or is liquidated.
3. Add a "Protect" button to the position view and a small "Protected" badge. TOSBI supplies the quote API and settlement.
4. Agree the fee split and the parameter change process (24-hour notice on any risk-parameter change; joint control over pause).

---

## 13. Frequently asked questions

**Can a trader cancel protection and get a refund?**
No. Once bought, protection either pays at settlement or expires. Refunds would let traders buy, watch, and cancel when the market goes their way.

**What if the trader's position is liquidated?**
Protection survives liquidation and settles at the liquidation price. This is often exactly when the trader needs it.

**What if the oracle goes down?**
Settlement waits for a valid price. If the outage is prolonged, contracts settle on the last valid average and new sales pause until the feed recovers.

**Who holds the capital?**
A smart contract on Stellar. The PerpDEX retains ownership and withdrawal rights; TOSBI cannot move the funds except to pay contracted payouts.

**Does TOSBI take risk?**
Not in v0.1. TOSBI earns the Protection Fee. The PerpDEX's capital earns the premiums and bears the payouts. Tranched vaults in v2 let other parties share that risk and return.

**Why Stellar?**
Fast (5-second) finality, negligible fees, native USDC, a production-grade oracle (Reflector), and an emerging set of perpetual DEXs with no protection layer yet. TOSBI can be the only options-equivalent on the chain.

---

*Technical specification, formulas, contract interfaces, and academic references are in the companion document "TOSBI — Architecture 1 Build Specification."*

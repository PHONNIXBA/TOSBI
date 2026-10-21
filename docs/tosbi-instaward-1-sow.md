# Instawards Statement of Work (SOW)
## 30-Day Scoped Engagement

---

## 1. Project & Team Information

| Field | Value |
|---|---|
| **Project Name** | TOSBI — Position Protection Infrastructure for PerpDEXs on Stellar |
| **Builder / Team Name** | James Dzung<br>*[add remaining team members]* |
| **Primary Contact (Name + Email)** | *[name — email]* |
| **Wallet Address (Stellar)** | *[G… address for disbursement]* |
| **Ambassador Chapter** | Stellar Vietnam Chapter |
| **Ambassador Chapter Lead** | Tiffany |
| **Date Submitted** | September 13, 2026 |
| **Suggested Sprint Start Date** | September 14, 2026 |

---

## 2. Instawards Overview & Intent

### 2.1 Instawards Purpose (for Builder Context)

Instawards are designed to support short, clearly scoped, execution-focused work that helps a project make tangible progress toward building on Stellar. Instawards are meant to fund specific, achievable outcomes that can be completed and demonstrated within 30 days or less.

This SOW is one of three coordinated Instawards (each $5,000) covering the on-chain foundation for TOSBI's full SCF Build Award. The three Instawards together produce a working, end-to-end protection product on Stellar testnet, broken into smaller, independently verifiable packages to enable easier internal review by the Stellar team.

- **Instaward 1 (this SOW, Days 1–10)** — Protection Capital vault and protection-minting contract, with the full-backing solvency invariant enforced on-chain, plus a signed-quote pricer bootstrap.
- **Instaward 2 (Days 11–20)** — Oracle adapter (Reflector SEP-40 TWAP), settlement engine, payout path, and keeper.
- **Instaward 3 (Days 21–30)** — PerpDEX position adapter, early settlement on position close/liquidation, and the concentration/utilization risk caps.

---

## 3. Problem Statement & Objective

### Problem Being Addressed
*What specific problem, gap, or blocker is this Instaward intended to solve?*

Stellar's first perpetual DEXs are arriving, but the chain has no options venue and no protection layer, so a Stellar perp trader has no way to cap downside on a leveraged position other than a stop-loss. Centralised exchanges solved this (Bybit Perp Protect), but no on-chain equivalent exists on any chain, let alone Stellar.

Building that layer requires two foundations to land first: (1) an on-chain **Protection Capital vault** that can hold a PerpDEX's USDC, reserve the maximum payout of every live contract, and **refuse to sell protection it cannot pay** — the full-backing invariant that makes the system solvent by construction; and (2) a **signed-quote pricer** that computes a put-spread premium off-chain from a calibrated volatility table and whose signature the contract can verify, so pricing is auditable and cannot be forged. Neither exists on Stellar today. Without both in Days 1–10, the subsequent Instawards have nothing to settle against.

### Objective of This Instaward
*In one or two sentences, what will be true at the end of this Instaward if it is successful?*

At the end of this 10-day window, two foundations are in place: (a) a working **Protection Vault + Book MVP** live on Stellar testnet with a visual demo where a reviewer connects Freighter, funds the pool, and clicks **Buy Protection** — watching reserved capital rise and the contract reject a purchase that would exceed the pool; and (b) a **pricer bootstrap** — an off-chain quote service with an empirically calibrated XLM volatility table that signs quotes with an ed25519 key, verified on-chain at mint. MVP = something touchable and visual.

---

## 4. Scope of Work (Instaward Deliverables)

> **Important guidance:** This scope must be achievable within 30 calendar days. This Instaward is scoped to a 10-day window (Days 1–10) so that the work is independently demonstrable.

### 4.1 In-Scope Deliverables

| | Description (What will be built or produced?) | Why this matters |
|---|---|---|
| **Deliverable 1** | **Protection Vault + Book Soroban contract (MVP)** — `deposit_capital`, `buy_protection`, `reserve`, `release`, and `withdraw_request` entry points in Rust. State transitions enforce authorisation, the **full-backing invariant** (Σ active MaxPayout ≤ available capital), and per-wallet and per-contract size limits. Deployed to Stellar testnet with a published contract ID. | The vault is the solvency anchor of TOSBI. Without an on-chain guarantee that reserved capital can never exceed the pool, a PerpDEX cannot safely commit capital to back trader payouts. |
| **Deliverable 2** | **Visual demo page (touchable MVP)** — single-page web app on free hosting (Cloudflare Pages / GitHub Pages). Reviewer connects Freighter, sees live vault state (total capital / reserved / free), clicks **Buy Protection** (primary user flow) plus admin buttons deposit-capital / release / withdraw-request. Includes an **"exceed the pool"** button that demonstrates the contract rejecting an over-committed purchase. Each action returns a tx-hash receipt linked to Stellar Expert. | The Ambassador Chapter Lead and Stellar reviewers (per SOW Section 6 — minimal technical expertise) can verify the MVP works by clicking, not by reading Rust or block-explorer tx hashes. The rejection button makes the solvency guarantee *visible*. |
| **Deliverable 3** | **Pricer bootstrap (quote service + signature verification)** — off-chain service that calibrates an XLM volatility table across the seven duration buckets (15m / 1h / 4h / 12h / 1d / 3d / 7d) from historical price data, computes the put-spread premium, and signs the quote payload with an ed25519 key. Contract verifies the signature, quote TTL, and nonce at mint. Vol table and calibration script committed to repo. Not yet wired to a live oracle — that happens in Instaward 2. | De-risks the pricing path early. By Day 10 the team has proven a premium can be computed, signed, carried on-chain and verified, so Instaward 2 can wire settlement and Instaward 3 can wire the PerpDEX on top without re-doing pricing work. |
| **Deliverable 4** | **9 passing test cases for the Vault + Book MVP** — happy-path deposit-capital → buy-protection → release, full-backing rejection, per-wallet cap rejection, unauthorised withdrawal rejection, expired-quote rejection, replayed-nonce rejection, and 3 edge cases. `cargo test` output attached. | Automated proof that the MVP behaves correctly under both happy-path and adversarial conditions — in particular that the solvency invariant holds under every ordering of operations. |

### Out-of-Scope (Explicitly Not Included)

*List anything that might be assumed but is not included in this Instaward scope.*

- Oracle adapter and Reflector TWAP integration (covered in Instaward 2)
- Settlement engine and payout computation (covered in Instaward 2)
- Keeper service for expiry settlement (covered in Instaward 2)
- PerpDEX position adapter and position binding (covered in Instaward 3)
- Early settlement on position close or liquidation (covered in Instaward 3)
- Concentration and utilization premium loadings enforced on-chain (covered in Instaward 3)
- SVI volatility surface and event-calendar surcharges (Build Award Tranche 2)
- Tranched vault with public senior capital (Build Award)
- Streaming / perpetual protection product (Build Award)
- Delta-hedged underwriting (deferred until a deep Stellar perp venue exists)
- Extensive documentation, CI/CD pipelines, or polished repo tooling — focus is the working MVP
- Frontend / web app UI beyond the demo page
- Backend/indexer services, analytics dashboards
- Security audit (planned for Build Award Tranche 3)
- Mainnet deployment

### 4.2 Deliverable-Aligned Budget Request

| Requested Budget Amount | Rationale for Budget Request |
|---|---|
| **$5,000** | Covers 10 days of focused development to lay both foundations the next two Instawards build on: (a) the Soroban / Rust Protection Vault + Book MVP with its full-backing invariant and visual demo page, and (b) the pricer bootstrap with a calibrated volatility table and on-chain signature verification. |

### 4.3 Cost Breakdown (How the $5,000 will be used)

*Detailed allocation of the $5,000 across work categories. Each line item maps to a concrete deliverable outcome.*

| Cost Line Item | Amount | Outcome / What the spend produces |
|---|---:|---|
| Protection Vault + Book Contract Development (MVP) | $2,000 | Working Soroban contract: deposit-capital / buy-protection / reserve / release / withdraw-request entry points in Rust, with the full-backing invariant, size caps, and authorisation checks. Compiles, deploys to Stellar testnet, callable via Soroban CLI. |
| Visual Demo Page (Buy Protection + admin buttons, touchable MVP) | $1,200 | Single-page web app on free hosting. Reviewer connects Freighter, sees live vault state, clicks Buy Protection + admin buttons, triggers the "exceed the pool" rejection, and gets tx-hash receipts linked to Stellar Expert. |
| Pricer Bootstrap (vol table + signed quotes + on-chain verification) | $1,200 | Off-chain quote service with an empirically calibrated XLM volatility table across seven duration buckets, put-spread premium computation, ed25519 quote signing, and on-chain signature / TTL / nonce verification. Calibration script and table committed to repo. |
| Testing (9 unit + integration tests) | $400 | 9 passing test cases for the Vault + Book MVP covering happy path, solvency rejection, caps, unauthorised actions, quote replay, and edge cases. `cargo test` output published. |
| Testnet Deployment & Evidence | $200 | MVP deployed on Stellar testnet with a public contract ID and deployment tx hash. Demo page URL + contract ID + pricer public key handed off for review. |
| **Total** | **$5,000** | |

---

## 5. Execution Plan & Timeline

### 5.1 Sprint Breakdown

This Instaward runs over a 10-day window (Days 1–10 of the overall 30-day engagement). Timing is set so the three Instawards can be executed sequentially or in tight overlap by the same team.

| Window | Planned Work | Expected Output |
|---|---|---|
| **Days 1–3** | Contract architecture: state types, storage layout (persistent storage + TTL strategy), entry-point signatures. Implement `deposit_capital` and the reserve/release accounting. Begin pricer bring-up: pull XLM historical price data, build the bucket calibration script. | Contract skeleton compiles. `deposit_capital` works end-to-end. Vol table produced for all seven buckets with a documented calibration method. |
| **Days 4–7** | Implement `buy_protection` with the full-backing invariant, per-wallet and per-contract caps, and quote-signature verification. Deploy to testnet. Build the pricer service: premium computation and ed25519 quote signing against the live vault state. | MVP live on testnet. A signed quote produced off-chain is accepted on-chain and mints a protection record; an over-committing quote is rejected. Demo page reachable on staging URL with wallet connect + Buy Protection working. |
| **Days 8–10** | Finish demo page (admin buttons, vault state display, "exceed the pool" rejection demo, tx-hash receipts). Complete 9-test suite. Final testnet deployment. Commit pricer source, vol table and calibration script to repo. Hand off demo URL + contract ID + pricer public key to Instaward 2. | 9/9 tests passing. Demo page deployed to public URL — reviewer can Buy Protection end-to-end and watch a rejection. Pricer public key published. Ready to hand off to Instaward 2. |

---

## 6. Evidence of Completion (Required)

> **Important guidance:** Evidence should be clear, verifiable, and easy to review by the Ambassador Chapter Lead with minimal technical expertise.

### 6.1 Planned Evidence to Be Submitted

| | Evidence Type (link, repo, demo, screenshot, doc, tx hash, etc.) | Description |
|---|---|---|
| **Deliverable 1** | Visual demo page URL (Buy Protection, touchable MVP) | Public URL a reviewer can open in any browser to connect Freighter and interact with the live Protection Vault MVP on Stellar testnet. Screenshot of Buy Protection + admin buttons + vault state display attached, including the rejection message when a purchase would exceed the pool. |
| **Deliverable 2** | Stellar testnet contract ID + deployment tx hash | Vault + Book MVP deployment transaction hash on Stellar testnet. Contract ID reachable on a public Stellar explorer (Stellar Lab / Stellar Expert). |
| **Deliverable 3** | Pricer public key + signed quote sample + vol table in repo | The pricer's ed25519 public key, one sample signed quote payload with the tx hash of the mint it produced, and the calibrated volatility table plus calibration script committed to repo. Demonstrates the pricing path is operational and ready for Instaward 2 to wire to settlement. |
| **Deliverable 4** | `cargo test` output (9/9 passing) | Full `cargo test` output showing all 9 tests passing. Attached to the deliverable submission. |

### 6.2 Evidence Verification Checklist (For Ambassador Use)

*For each deliverable, the Ambassador Chapter Lead will assess whether evidence is present and sufficient.*

| Deliverable | Evidence Present | Evidence Partial | Evidence Missing | Comments |
|---|:--:|:--:|:--:|---|
| **Deliverable 1** — Visual demo page URL (Buy Protection, touchable MVP) | [ ] | [ ] | [ ] | |
| **Deliverable 2** — Stellar testnet contract ID + deployment tx hash | [ ] | [ ] | [ ] | |
| **Deliverable 3** — Pricer public key + signed quote sample + vol table | [ ] | [ ] | [ ] | |
| **Deliverable 4** — `cargo test` output (9/9 passing) | [ ] | [ ] | [ ] | |

---

## 7. Next-Step Alignment

### 7.1 Anticipated Next Step After Completion

After this Instaward, the most likely next step is:

- [ ] Apply to SCF Build Award
- [X] **Apply for a follow-on Instaward:** Instaward 2 (Reflector SEP-40 oracle adapter, settlement engine computing the loss-linked capped payout from a duration-scaled TWAP, payout path, and expiry keeper) will be submitted immediately after this one as the planned follow-on.
- [ ] Continue development independently
- [ ] Seek other ecosystem support
- [ ] Other:

---

## 8. Instawards Constraints Acknowledgement

By submitting this SOW, the Builder acknowledges:

- [X] This scope will be completed within 30 days or less.
- [X] Instawards support execution, not open-ended exploration.
- [X] A project may receive no more than two follow-on Instawards.
- [X] Each Instaward is capped at $5,000.
- [X] Total Instawards funding may not exceed $15,000.

---

## 9. Submission Confirmation

Once finalized, this Statement of Work will be submitted by the Ambassador Chapter Lead via the Instawards Airtable submission form for review and approval. Funds will be disbursed to the wallet address listed in Section 1.

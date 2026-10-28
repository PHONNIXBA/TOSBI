import { test } from "node:test";
import assert from "node:assert/strict";
import { Keypair } from "@stellar/stellar-sdk";
import { BadRequest, makeQuote, Upstream, UNIT, type Deps, type Position } from "../src/service.ts";

const kp = Keypair.random();
const owner = Keypair.random().publicKey();
const POS = "ab".repeat(32);
const position = (over: Partial<Position> = {}): Position => ({
  owner,
  asset: "XLM",
  direction: 0,
  size: 5_000n * UNIT,
  mark_price: 2_000_000n,
  status: 0,
  ...over,
});
const deps = (over: Partial<Deps> = {}): Deps => ({
  secret: kp.secret(),
  vault: "CAAQCAIBAEAQCAIBAEAQCAIBAEAQCAIBAEAQCAIBAEAQCAIBAEAQC526",
  perpdex: "CABQGAYDAMBQGAYDAMBQGAYDAMBQGAYDAMBQGAYDAMBQGAYDAMBQGCK3",
  volTables: { XLM: { buckets: { "3600": { down: 0.7, up: 0.7 }, "86400": { down: 1.0, up: 0.9 } } } },
  now: () => 1_000n,
  nonce: () => 7n,
  position: async () => position(),
  vaultState: async () => ({ capital: 500_000n * UNIT, reserved: 249_750n * UNIT, locked: 0n }),
  exposure: async (_a, dir) => (dir === "long" ? 99_750n * UNIT : 0n),
  depth: async () => null,
  shock: async () => null,
  ...over,
});
const req = { positionId: POS, amount: "1000", duration: 86_400, rangeBps: 2_500 };

test("quote reproduces spec §10.1 from the position's mark and on-chain exposure", async () => {
  const q = await makeQuote(req, deps());
  assert.equal(q.quote.trader, owner);
  assert.equal(q.quote.position_id, POS);
  assert.equal(q.quote.p_start, 2_000_000n);
  assert.equal(q.quote.p_end, 1_500_000n);
  assert.equal(q.quote.qty, 5_000n * UNIT);
  assert.equal(q.quote.max_payout, 250n * UNIT);
  assert.equal(q.quote.fee, 3n * UNIT);
  assert.equal(q.quote.expires_at, 1_030n);
  assert.equal(q.breakdown.utilization, 0.5);
  assert.equal(q.breakdown.concentration, 0.2);
  assert.ok(Math.abs(Number(q.quote.premium) / 1e7 - 25.64) < 0.01);
  assert.ok(kp.verify(Buffer.from(q.xdr, "base64"), Buffer.from(q.signature, "hex")));
});

test("a short position gets SHORT cover priced from σ_up", async () => {
  const q = await makeQuote(req, deps({ position: async () => position({ direction: 1 }) }));
  assert.equal(q.quote.direction, "short");
  assert.equal(q.quote.p_end, 2_500_000n);
  assert.equal(q.quote.vol_bps, 9_000);
});

test("locked capital is not counted as available", async () => {
  const q = await makeQuote(req, deps({ vaultState: async () => ({ capital: 600_000n * UNIT, reserved: 249_750n * UNIT, locked: 100_000n * UNIT }) }));
  assert.equal(q.breakdown.utilization, 0.5);
});

test("bad inputs are rejected with BadRequest", async () => {
  const bad = [
    { ...req, positionId: "nope" },
    { ...req, positionId: POS.toUpperCase() },
    { ...req, duration: 600 },
    { ...req, duration: "86400" },
    { ...req, rangeBps: 499 },
    { ...req, rangeBps: 5_001 },
    { ...req, amount: 1000 },
    { ...req, amount: "0" },
    { ...req, amount: "50000.0000001" },
    { ...req, amount: "1000.0000001" },
  ];
  for (const input of bad) await assert.rejects(makeQuote(input, deps()), BadRequest, JSON.stringify(input));
});

test("cover larger than the position, closed positions and unlisted assets are refused", async () => {
  await assert.rejects(makeQuote({ ...req, amount: "1000.01" }, deps()), BadRequest);
  await assert.rejects(makeQuote(req, deps({ position: async () => position({ status: 1 }) })), BadRequest);
  await assert.rejects(makeQuote(req, deps({ position: async () => position({ asset: "DOGE" }) })), BadRequest);
});

test("liquidity cap refuses cover once exposure passes κ × depth", async () => {
  // exposure 99,750 + new 250 = 100,000 → needs depth ≥ 200,000
  await makeQuote(req, deps({ depth: async () => 200_000 }));
  await assert.rejects(makeQuote(req, deps({ depth: async () => 199_999 })), /liquidity cap/);
});

test("vol shock pauses ≤ 1h covers and scales σ for longer ones", async () => {
  const shock = async () => ({ factor: 2.5, until: 1_900 });
  await assert.rejects(makeQuote({ ...req, duration: 3_600 }, deps({ shock })), Upstream);
  const q = await makeQuote(req, deps({ shock }));
  assert.equal(q.breakdown.sigma, 2.5);
  const expired = async () => ({ factor: 2.5, until: 999 });
  assert.equal((await makeQuote(req, deps({ shock: expired }))).breakdown.sigma, 1.0);
});

test("no quote is signed when the chain read fails", async () => {
  await assert.rejects(makeQuote(req, deps({ position: async () => { throw new Error("rpc"); } })), Upstream);
  await assert.rejects(makeQuote(req, deps({ vaultState: async () => { throw new Error("rpc"); } })), Upstream);
  await assert.rejects(makeQuote(req, deps({ exposure: async () => { throw new Error("rpc"); } })), Upstream);
});

test("the secret never appears in the response", async () => {
  const q = await makeQuote(req, deps());
  const body = JSON.stringify(q, (_, v) => (typeof v === "bigint" ? v.toString() : v));
  assert.ok(!body.includes(kp.secret()));
});

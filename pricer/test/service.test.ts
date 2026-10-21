import { test } from "node:test";
import assert from "node:assert/strict";
import { Keypair } from "@stellar/stellar-sdk";
import { BadRequest, makeQuote, Upstream, UNIT, type Deps } from "../src/service.ts";

const kp = Keypair.random();
const trader = Keypair.random().publicKey();
const deps = (over: Partial<Deps> = {}): Deps => ({
  secret: kp.secret(),
  vault: "CAAQCAIBAEAQCAIBAEAQCAIBAEAQCAIBAEAQCAIBAEAQCAIBAEAQC526",
  volTable: { buckets: { "86400": { down: 1.0, up: 0.9 } } },
  now: () => 1_000n,
  nonce: () => 7n,
  spot: async () => 0.2,
  vaultState: async () => ({ capital: 500_000n * UNIT, reserved: 249_750n * UNIT }),
  ...over,
});
const req = { trader, direction: "long", amount: "1000", duration: 86_400, rangeBps: 2_500 };

test("quote matches spec §10.1 terms and verifies under the pricer key", async () => {
  const q = await makeQuote(req, deps());
  assert.equal(q.quote.p_start, 2_000_000n);
  assert.equal(q.quote.p_end, 1_500_000n);
  assert.equal(q.quote.qty, 5_000n * UNIT);
  assert.equal(q.quote.max_payout, 250n * UNIT);
  assert.equal(q.quote.fee, 3n * UNIT);
  assert.equal(q.quote.expires_at, 1_030n);
  assert.equal(q.quote.vol_bps, 10_000);
  assert.equal(q.breakdown.utilization, 0.5);
  assert.ok(Math.abs(Number(q.quote.premium) / 1e7 - 20.87 * 1.125 * 1.05) < 0.01);
  assert.ok(kp.verify(Buffer.from(q.xdr, "base64"), Buffer.from(q.signature, "hex")));
});

test("SHORT quote covers a rise and uses σ_up", async () => {
  const q = await makeQuote({ ...req, direction: "short" }, deps());
  assert.equal(q.quote.p_end, 2_500_000n);
  assert.equal(q.quote.vol_bps, 9_000);
});

test("X11: bad inputs are rejected with BadRequest", async () => {
  const bad = [
    { ...req, trader: "nope" },
    { ...req, direction: "up" },
    { ...req, duration: 600 },
    { ...req, rangeBps: 499 },
    { ...req, rangeBps: 5_001 },
    { ...req, rangeBps: 25.5 },
    { ...req, amount: 1000 },
    { ...req, amount: "-5" },
    { ...req, amount: "1.12345678" },
    { ...req, amount: "0" },
    { ...req, amount: "50000.0000001" },
  ];
  for (const input of bad) await assert.rejects(makeQuote(input, deps()), BadRequest, JSON.stringify(input));
});

test("X12: no quote is signed when the price feed or vault read fails", async () => {
  await assert.rejects(makeQuote(req, deps({ spot: async () => { throw new Error("down"); } })), Upstream);
  await assert.rejects(makeQuote(req, deps({ spot: async () => NaN })), Upstream);
  await assert.rejects(makeQuote(req, deps({ vaultState: async () => { throw new Error("rpc"); } })), Upstream);
});

test("E10: empty pool still yields a signed quote for the contract to reject", async () => {
  const q = await makeQuote(req, deps({ vaultState: async () => ({ capital: 0n, reserved: 0n }) }));
  assert.equal(q.breakdown.mU, 1.5);
});

test("S9: the secret never appears in the response", async () => {
  const q = await makeQuote(req, deps());
  const body = JSON.stringify(q, (_, v) => (typeof v === "bigint" ? v.toString() : v));
  assert.ok(!body.includes(kp.secret()));
});

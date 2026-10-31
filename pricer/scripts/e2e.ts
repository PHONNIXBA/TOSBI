// End-to-end run of the Worker API against a deployed vault. Run scripts/deploy.sh first.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { Keypair, TransactionBuilder } from "@stellar/stellar-sdk";
import worker from "../src/worker.ts";

type Env = Parameters<typeof worker.fetch>[1];
const vars = readFileSync(new URL("../.dev.vars", import.meta.url), "utf8").trim().split("\n");
const store = new Map<string, string>();
const kv = {
  async get(key: string): Promise<string | null> {
    return store.get(key) ?? null
  },
  async put(key: string, value:string, _options?: {expirationTtl?: number}) : Promise<void> {
    store.set(key, value);
  }
}
const env = {
  ...Object.fromEntries(vars.map((l) => [l.slice(0, l.indexOf("=")), l.slice(l.indexOf("=") + 1)])),
  KV: kv
} as unknown as Env
const friendbot = env.RPC_URL.includes("localhost") ? "http://localhost:8000/friendbot" : "https://friendbot.stellar.org";
const U = 10_000_000;
type Body = any;

async function call(path: string, body?: object): Promise<{ status: number; body: Body }> {
  const init = body ? { method: "POST", body: JSON.stringify(body) } : undefined;
  const res = await worker.fetch(new Request(`http://demo${path}`, init), env);
  return { status: res.status, body: await res.json() };
}

async function signSubmit(kp: Keypair, xdr: string): Promise<Body> {
  const tx = TransactionBuilder.fromXDR(xdr, env.NETWORK_PASSPHRASE);
  tx.sign(kp);
  const r = await call("/api/submit", { xdr: tx.toXDR() });
  assert.equal(r.body.status, "SUCCESS", JSON.stringify(r.body));
  return r.body;
}

const state = async (who: string) => (await call(`/api/state?addr=${who}`)).body;
const ok = (name: string) => console.log(`ok  ${name}`);

const trader = Keypair.random();
const me = trader.publicKey();
assert.ok((await fetch(`${friendbot}?addr=${me}`)).ok, "friendbot");
const s0 = await state(me);
assert.ok(Number(s0.state.capital) >= 10_000 * U);
assert.equal(s0.wallet.balance, null);

await signSubmit(trader, (await call("/api/tx", { action: "trust", source: me })).body.tx);
const fauceted = await call("/api/faucet", { addr: me });
assert.equal(fauceted.body.status, "SUCCESS");
assert.equal((await state(me)).wallet.balance, String(1_000 * U));
assert.equal((await call("/api/faucet", { addr: me })).status, 400);
ok("trustline + faucet 1,000 tUSDC, second faucet refused");

async function openPosition(asset: "XLM" | "BTC" | "ETH", direction: "long" | "short", notional: string): Promise<string> {
  const response = await call("/api/tx", {
    action: "open",
    source: me,
    asset,
    direction,
    notional
  });
  assert.ok(response.body.tx, JSON.stringify(response.body));
  const submitted = await signSubmit(trader, response.body.tx);
  const positionId = String(submitted.result);
  assert.match(positionId, /^[0-9a-f]{64}$/, "Mock PerpDex must return a 32-byte hex position ID");
  return positionId;
}
const pos0 = await openPosition("XLM", "long", "1000");
const pos1 = await openPosition("XLM", "long", "2000");
const pos2 = await openPosition("XLM", "long", "60000");
const pos3 = await openPosition("XLM", "short", "300");
ok("opened 4 positions: LONG 1000/2000/60000, SHORT 300");

const configState = await state(me);
const minPositionAge = Number(configState.config.params.min_position_age);
assert.ok(Number.isSafeInteger(minPositionAge) && minPositionAge >= 0, "invalid min_position_age");
if (minPositionAge > 0) {
  console.log(`Waiting ${minPositionAge}s for positions to mature...`);
  await new Promise<void>((resolve) => setTimeout(resolve, minPositionAge * 1000));
}
ok(`position age requirement satisfied (${minPositionAge}s)`);

const beforeBuy = await state(me);
const t0 = Date.now();
const q = await call("/api/quote", { positionId: pos0, amount: "500", duration: 86_400, rangeBps: 2_500 });
const quoteMs = Date.now() - t0;
const id0 = String((await signSubmit(trader, q.body.tx)).result);
const afterBuy = await state(me);
assert.equal(BigInt(afterBuy.state.reserved) - BigInt(beforeBuy.state.reserved), BigInt(q.body.quote.max_payout));
ok(`bought LONG protection #${id0}: premium ${Number(q.body.quote.premium) / U}, fee ${Number(q.body.quote.fee) / U} (quote in ${quoteMs} ms)`);

const duplicate = await call("/api/quote", { positionId: pos0, amount: "500", duration: 86_400, rangeBps: 2_500 });
assert.equal(duplicate.body.rejected?.name, "AlreadyCovered", JSON.stringify(duplicate.body));
ok("duplicate protection rejected: AlreadyCovered");

const capped = await call("/api/quote", { positionId: pos1, amount: "400", duration: 86_400, rangeBps: 2_500 });
assert.equal(capped.body.rejected?.name, "WalletCapExceeded", JSON.stringify(capped.body));
ok("second position protection rejected: WalletCapExceeded");

const exceed = await call("/api/quote", { positionId: pos2, amount: "50000", duration: 86_400, rangeBps: 5_000 });
assert.equal(exceed.body.rejected?.name, "InsufficientCapital");
ok("exceed-the-pool quote rejected by the contract: InsufficientCapital");

const overPosition = await call("/api/quote", { positionId: pos0, amount: "1001", duration: 86_400, rangeBps: 2_500 });
assert.equal(overPosition.status, 400, JSON.stringify(overPosition.body));
ok("protection amount exceeding position notional rejected: HTTP 400");

const short = await call("/api/quote", { positionId: pos3, amount: "200", duration: 3_600, rangeBps: 1_000 });
const id1 = String((await signSubmit(trader, short.body.tx)).result);
const p1 = (await call(`/api/protection?id=${id1}`)).body;
assert.equal(p1.direction, 1);
assert.ok(BigInt(p1.p_end) > BigInt(p1.p_start));
ok(`bought SHORT protection #${id1} covering a rise`);

const wd = await call("/api/tx", { action: "withdraw_request", source: me, amount: "1" });
assert.deepEqual(wd.body.rejected.needs, [s0.config.owner], JSON.stringify(wd.body));
ok("withdrawal requires Owner authorization");

const pause = await call("/api/tx", { action: "pause", source: me });
assert.equal(pause.body.rejected?.name, "Unauthorized", JSON.stringify(pause.body));
ok("unauthorized pause rejected: Unauthorized");

const early = await call("/api/tx", { action: "settle_early", source: me, id: id0 });
assert.equal(early.body.rejected?.name, "NotClosed", JSON.stringify(early.body));
ok("early settlement rejected for open position: NotClosed");

const beforeSettlement = (await call(`/api/protection?id=${id0}`)).body;
assert.equal(beforeSettlement.status, 0, "protection must be Active before settlement");
await signSubmit(trader, (await call("/api/tx", { action: "close", source: me, positionId: pos0 })).body.tx);
const pending: Promise<unknown>[] = [];
const ctx = {
  waitUntil(promise: Promise<unknown>): void {
    pending.push(promise);
  },
  passThroughOnException(): void {},
} as unknown as ExecutionContext;
await worker.scheduled({cron: "* * * * *"} as ScheduledController, env, ctx);
assert.ok(pending.length > 0, "scheduled Worker registered no tasks");
await Promise.all(pending);
const afterSettlement = (await call(`/api/protection?id=${id0}`)).body;
assert.equal(afterSettlement.status, 1, JSON.stringify(afterSettlement));
ok("closed first position and Keeper settled protection");

const beforeDeposit = await state(me);
await signSubmit(trader, (await call("/api/tx", { action: "deposit", source: me, amount: "100" })).body.tx);
const afterDeposit = await state(me);
assert.equal(BigInt(afterDeposit.state.capital) - BigInt(beforeDeposit.state.capital), BigInt(100 * U));
ok("deposit of 100 tUSDC increased pool capital");

const malformedRequests = [
  { positionId: pos1, amount: "-1", duration: 86_400, rangeBps: 2_500 },
  { positionId: pos1, amount: "100", duration: 86_400, rangeBps: 0 },
  { positionId: "invalid", amount: "100", duration: 86_400, rangeBps: 2_500 }
];
for (const request of malformedRequests) {
  const response = await call("/api/quote", request);
  assert.equal(response.status, 400, JSON.stringify({request, response}));
}

for (const [path, body] of [
  ["/api/quote", { trader: me, direction: "long", amount: "abc", duration: 86_400, rangeBps: 2_500 }],
  ["/api/tx", { action: "nuke", source: me }],
  ["/api/tx", { action: "deposit", source: "not-an-address", amount: "1" }],
] as const) {
  assert.equal((await call(path, body)).status, 400, path);
}
assert.equal((await call("/api/protection?id=x")).status, 400);
ok("malformed requests return 400");

console.log("\nE2E passed");

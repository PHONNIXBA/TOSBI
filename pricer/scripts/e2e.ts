// End-to-end run of the Worker API against a deployed vault. Run scripts/deploy.sh first.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { Keypair, TransactionBuilder } from "@stellar/stellar-sdk";
import worker from "../src/worker.ts";

type Env = Parameters<typeof worker.fetch>[1];
const vars = readFileSync(new URL("../.dev.vars", import.meta.url), "utf8").trim().split("\n");
const env = Object.fromEntries(vars.map((l) => [l.slice(0, l.indexOf("=")), l.slice(l.indexOf("=") + 1)])) as unknown as Env;
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
ok("vault readable, new wallet has no trustline");

await signSubmit(trader, (await call("/api/tx", { action: "trust", source: me })).body.tx);
const fauceted = await call("/api/faucet", { addr: me });
assert.equal(fauceted.body.status, "SUCCESS");
assert.equal((await state(me)).wallet.balance, String(1_000 * U));
assert.equal((await call("/api/faucet", { addr: me })).status, 400);
ok("trustline + faucet 1,000 tUSDC, second faucet refused");

const t0 = Date.now();
const q = await call("/api/quote", { trader: me, direction: "long", amount: "500", duration: 86_400, rangeBps: 2_500 });
const quoteMs = Date.now() - t0;
assert.ok(q.body.tx, JSON.stringify(q.body));
const bought = await signSubmit(trader, q.body.tx);
const id0 = String(bought.result);
const s1 = await state(me);
assert.equal(s1.wallet.reserved, String(125 * U));
assert.equal(BigInt(s1.state.reserved) - BigInt(s0.state.reserved), BigInt(125 * U));
assert.equal(BigInt(s1.state.capital) - BigInt(s0.state.capital), BigInt(q.body.quote.premium));
ok(`bought LONG protection #${id0}: premium ${Number(q.body.quote.premium) / U}, fee ${Number(q.body.quote.fee) / U} (quote in ${quoteMs} ms)`);

const exceed = await call("/api/quote", { trader: me, direction: "long", amount: "50000", duration: 86_400, rangeBps: 5_000 });
assert.equal(exceed.body.rejected?.name, "InsufficientCapital");
ok("exceed-the-pool quote rejected by the contract: InsufficientCapital");

const capped = await call("/api/quote", { trader: me, direction: "long", amount: "400", duration: 86_400, rangeBps: 2_500 });
assert.equal(capped.body.rejected?.name, "WalletCapExceeded");
ok("second purchase over the 2% wallet cap rejected: WalletCapExceeded");

const short = await call("/api/quote", { trader: me, direction: "short", amount: "200", duration: 3_600, rangeBps: 1_000 });
const id1 = String((await signSubmit(trader, short.body.tx)).result);
const p1 = (await call(`/api/protection?id=${id1}`)).body;
assert.equal(p1.direction, 1);
assert.ok(BigInt(p1.p_end) > BigInt(p1.p_start));
ok(`bought SHORT protection #${id1} covering a rise`);

const wd = await call("/api/tx", { action: "withdraw_request", source: me, amount: "1" });
assert.equal(wd.body.rejected?.name, "AuthRequired");
assert.deepEqual(wd.body.rejected.needs, [s0.config.admin]);
ok("non-admin withdrawal refused: contract requires the admin's signature");

await signSubmit(trader, (await call("/api/tx", { action: "release", source: me, id: id0 })).body.tx);
const s2 = await state(me);
assert.equal(BigInt(s1.state.reserved) - BigInt(s2.state.reserved), BigInt(125 * U) - BigInt(p1.max_payout));
assert.equal((await call("/api/tx", { action: "release", source: me, id: id0 })).body.rejected?.name, "AlreadyReleased");
ok(`released #${id0}; releasing again rejected: AlreadyReleased`);

await signSubmit(trader, (await call("/api/tx", { action: "deposit", source: me, amount: "100" })).body.tx);
assert.equal(BigInt((await state(me)).state.capital) - BigInt(s2.state.capital), BigInt(100 * U));
ok("deposit of 100 tUSDC increased pool capital");

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

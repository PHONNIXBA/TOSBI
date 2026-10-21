import { Address, Asset, nativeToScVal, StrKey, xdr } from "@stellar/stellar-sdk";
import volTable from "../vol-table.json" with { type: "json" };
import { pay, prepareInvoke, prepareTrust, read, Rejected, submit, type Net } from "./chain.ts";
import { BadRequest, makeQuote, parseUnits, randomNonce, Upstream } from "./service.ts";

interface Env {
  RPC_URL: string;
  NETWORK_PASSPHRASE: string;
  VAULT_ID: string;
  ASSET_CODE: string;
  ASSET_ISSUER: string;
  PRICER_SECRET: string;
  FAUCET_SECRET: string;
}

// Binance blocks Cloudflare egress, so it is the last resort.
const SPOT_SOURCES: [string, (j: any) => string][] = [
  ["https://api.coinbase.com/v2/prices/XLM-USD/spot", (j) => j.data.amount],
  ["https://api.kraken.com/0/public/Ticker?pair=XLMUSD", (j) => j.result.XXLMZUSD.c[0]],
  ["https://data-api.binance.vision/api/v3/ticker/price?symbol=XLMUSDT", (j) => j.price],
];
const FAUCET_AMOUNT = "1000";

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body, (_, v) => (typeof v === "bigint" ? v.toString() : v instanceof Uint8Array ? Buffer.from(v).toString("hex") : v)), {
    status,
    headers: { "content-type": "application/json" },
  });

const i128 = (amount: unknown) => nativeToScVal(parseUnits(amount), { type: "i128" });

function account(a: unknown): string {
  if (typeof a !== "string" || !StrKey.isValidEd25519PublicKey(a)) throw new BadRequest("expected a Stellar G... address");
  return a;
}
const addr = (a: unknown) => new Address(account(a)).toScVal();

function u64(id: unknown) {
  if (typeof id !== "string" || !/^\d{1,19}$/.test(id)) throw new BadRequest("id must be a non-negative integer");
  return nativeToScVal(BigInt(id), { type: "u64" });
}

async function spot(): Promise<number> {
  const failures: string[] = [];
  for (const [url, pick] of SPOT_SOURCES) {
    try {
      const res = await fetch(url);
      const price = res.ok ? Number(pick(await res.json())) : NaN;
      if (price > 0) return price;
      failures.push(`${new URL(url).host} ${res.status}`);
    } catch (e) {
      failures.push(`${new URL(url).host} ${(e as Error).message}`);
    }
  }
  throw new Error(`no price source reachable (${failures.join(", ")})`);
}

async function balance(n: Net, token: string, who: string): Promise<bigint | null> {
  try {
    return (await read(n, token, "balance", [addr(who)])) as bigint;
  } catch {
    return null;
  }
}

async function route(req: Request, env: Env): Promise<Response> {
  const n: Net = { rpcUrl: env.RPC_URL, passphrase: env.NETWORK_PASSPHRASE };
  const asset = new Asset(env.ASSET_CODE, env.ASSET_ISSUER);
  const token = asset.contractId(env.NETWORK_PASSPHRASE);
  const path = new URL(req.url).pathname;
  const body: Record<string, unknown> =
    req.method === "POST"
      ? await req.json().catch(() => {
          throw new BadRequest("invalid JSON body");
        })
      : {};

  if (path === "/api/state") {
    const q = new URL(req.url).searchParams.get("addr");
    const who = q ? account(q) : null;
    const [state, config] = await Promise.all([read(n, env.VAULT_ID, "state"), read(n, env.VAULT_ID, "config")]);
    const wallet = who
      ? { balance: await balance(n, token, who), reserved: await read(n, env.VAULT_ID, "wallet_reserved", [addr(who)]) }
      : null;
    const assetInfo = { code: env.ASSET_CODE, issuer: env.ASSET_ISSUER, contract: token };
    return json({ state, config, wallet, asset: assetInfo, vault: env.VAULT_ID, network: env.NETWORK_PASSPHRASE });
  }

  if (path === "/api/protection") {
    return json(await read(n, env.VAULT_ID, "protection", [u64(new URL(req.url).searchParams.get("id"))]));
  }

  if (path === "/api/quote") {
    const q = await makeQuote(body, {
      secret: env.PRICER_SECRET,
      vault: env.VAULT_ID,
      volTable,
      now: () => BigInt(Math.floor(Date.now() / 1000)),
      nonce: randomNonce,
      spot,
      vaultState: async () => (await read(n, env.VAULT_ID, "state")) as { capital: bigint; reserved: bigint },
    });
    const args = [xdr.ScVal.fromXDR(q.xdr, "base64"), nativeToScVal(Buffer.from(q.signature, "hex"))];
    const shown = { quote: q.quote, breakdown: q.breakdown, signature: q.signature };
    try {
      return json({ ...shown, tx: await prepareInvoke(n, q.quote.trader, env.VAULT_ID, "buy_protection", args) });
    } catch (e) {
      if (e instanceof Rejected) return json({ ...shown, rejected: { name: e.name, needs: e.needs } });
      throw e;
    }
  }

  if (path === "/api/tx") {
    const action = String(body.action);
    const source = account(body.source);
    if (action === "trust") return json({ tx: await prepareTrust(n, source, asset) });
    const call: Record<string, () => [string, xdr.ScVal[]]> = {
      deposit: () => ["deposit_capital", [addr(source), i128(body.amount)]],
      release: () => ["release", [addr(source), u64(body.id)]],
      withdraw_request: () => ["withdraw_request", [i128(body.amount)]],
      withdraw_execute: () => ["withdraw_execute", []],
    };
    if (!Object.hasOwn(call, action)) throw new BadRequest("unknown action");
    const [method, args] = call[action]();
    return json({ tx: await prepareInvoke(n, source, env.VAULT_ID, method, args) });
  }

  if (path === "/api/submit") {
    if (typeof body.xdr !== "string") throw new BadRequest("xdr required");
    return json(await submit(n, body.xdr));
  }

  if (path === "/api/faucet") {
    const to = account(body.addr);
    const bal = await balance(n, token, to);
    if (bal === null) throw new BadRequest(`add a trustline to ${env.ASSET_CODE} first`);
    if (bal >= parseUnits(FAUCET_AMOUNT)) throw new BadRequest(`you already hold ${FAUCET_AMOUNT}+ ${env.ASSET_CODE}`);
    return json(await pay(n, env.FAUCET_SECRET, asset, to, FAUCET_AMOUNT));
  }

  return json({ error: "not found" }, 404);
}

export default {
  async fetch(req: Request, env: Env): Promise<Response> {
    try {
      return await route(req, env);
    } catch (e) {
      if (e instanceof Rejected) return json({ rejected: { name: e.name, needs: e.needs, detail: e.message.slice(0, 400) } });
      if (e instanceof BadRequest) return json({ error: e.message }, 400);
      if (e instanceof Upstream) return json({ error: e.message }, 503);
      const msg = (e as Error).message ?? String(e);
      if (/Account not found/i.test(msg)) return json({ error: "this account does not exist on testnet yet; fund it with testnet XLM first" }, 400);
      return json({ error: msg }, 500);
    }
  },
};

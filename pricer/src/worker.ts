import { Address, Asset, nativeToScVal, StrKey, xdr } from "@stellar/stellar-sdk";
import volTables from "../vol-bundle.json" with { type: "json" };
import { pay, prepareInvoke, prepareTrust, read, Rejected, submit, invokeAs, type Net } from "./chain.ts";
import { BadRequest, makeQuote, parseUnits, randomNonce, Upstream, type Position, type Shock, type VolTable } from "./service.ts";
import { runKeeper, type KeptProtection } from "./keeper.ts";
import { recentCloses, realizedVol, depthUsd } from "./market.ts";

export interface KV {
  get(key: string): Promise<string | null>;
  put(key: string, value: string, opts?: {expirationTtl?: number}) : Promise<void>
}

interface Env {
  RPC_URL: string; 
  NETWORK_PASSPHRASE: string;
  VAULT_ID: string;
  ASSET_CODE: string;
  ASSET_ISSUER: string;
  PRICER_SECRET: string;
  FAUCET_SECRET: string;
  KV: KV;
  PERPDEX_ID: string;
  KEEPER_SECRET: string;
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

function positionId(value: unknown): xdr.ScVal {
  if (typeof value !== "string" || !/^[0-9a-fA-F]{64}$/.test(value)) {
    throw new BadRequest("positionId must be a 32-byte hex string");
  }
  return nativeToScVal(Buffer.from(value, "hex"));
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

async function getExposure(n: Net, vaultId: string, asset: string, direction: 0 | 1): Promise<unknown> {
  return read(n, vaultId, "exposure", [
    xdr.ScVal.scvSymbol(asset),
    xdr.ScVal.scvU32(direction)
  ]);
}

async function route(req: Request, env: Env): Promise<Response> {
  const n: Net = { rpcUrl: env.RPC_URL, passphrase: env.NETWORK_PASSPHRASE };
  const asset = new Asset(env.ASSET_CODE, env.ASSET_ISSUER);
  const token = asset.contractId(env.NETWORK_PASSPHRASE);
  const path = new URL(req.url).pathname;
  
  let body: Record<string, unknown> = {};
  if (req.method === "POST") {
    const parsed: unknown = await req.json().catch(() => {
      throw new BadRequest("Invalid JSON body");
    });

    if (
      typeof parsed !== "object" ||
      parsed === null ||
      Array.isArray(parsed)
    ) {
      throw new BadRequest("JSON body must be an object");
    }
    body = parsed as Record<string, unknown>;
  }

  if (path === "/api/state") {
    const q = new URL(req.url).searchParams.get("addr");
    const who = q ? account(q) : null;
    const [
      state, 
      config, 
      paused, 
      nextId,
      xlmLong,
      xlmShort,
      btcLong,
      btcShort,
      ethLong,
      ethShort  
    ] = await Promise.all([
      read(n, env.VAULT_ID, "state"), 
      read(n, env.VAULT_ID, "config"),
      read(n, env.VAULT_ID, ("paused")),
      read(n, env.VAULT_ID, "next_id"),
      getExposure(n, env.VAULT_ID, "XLM", 0),
      getExposure(n, env.VAULT_ID, "XLM", 1),
      getExposure(n, env.VAULT_ID, "BTC", 0),
      getExposure(n, env.VAULT_ID, "BTC", 1),
      getExposure(n, env.VAULT_ID, "ETH", 0),
      getExposure(n, env.VAULT_ID, "ETH", 1),
    ]);
    const wallet = who
      ? { balance: await balance(n, token, who), reserved: await read(n, env.VAULT_ID, "wallet_reserved", [addr(who)]) }
      : null;
    const assetInfo = { code: env.ASSET_CODE, issuer: env.ASSET_ISSUER, contract: token };
    return json({ 
      state, 
      config, 
      wallet, 
      asset: assetInfo, 
      vault: env.VAULT_ID, 
      network: env.NETWORK_PASSPHRASE,
      paused,
      nextId,
      perpdex: env.PERPDEX_ID,
      exposure: {
        XLM: {long: xlmLong, short: xlmShort},
        BTC: {long: btcLong, short: btcShort},
        ETH: {long: ethLong, short: ethShort}
      }
    });
  }

  if (path === "/api/protection") {
    return json(await read(n, env.VAULT_ID, "protection", [u64(new URL(req.url).searchParams.get("id"))]));
  }

  if (req.method === "GET" && path === "/api/position") {
    const id = new URL(req.url).searchParams.get("id");
    return json(await read(n, env.PERPDEX_ID, "position", [positionId(id)]));
  }

  if (req.method === "GET" && path === "/api/vol-tables") {
    const stored = await env.KV.get("vol-tables");
    const data = stored ?? JSON.stringify(volTables);
    return new Response(data, {
      headers: {
        "content-type": "application/json"
      }
    });
  }

  if (path === "/api/quote") {
    const q = await makeQuote(body, {
      secret: env.PRICER_SECRET,
      vault: env.VAULT_ID,
      perpdex: env.PERPDEX_ID,
      volTables,
      now: () => BigInt(Math.floor(Date.now() / 1000)),
      nonce: randomNonce,
      position: async (id) => (await read(n, env.PERPDEX_ID, "get_position", [positionId(id)])) as Position,
      vaultState: async () => (await read(n, env.VAULT_ID, "state")) as { capital: bigint; reserved: bigint; locked: bigint },
      exposure: async (asset, dir) => (await read(n, env.VAULT_ID, "exposure", [
        xdr.ScVal.scvSymbol(asset),
        xdr.ScVal.scvU32(dir === "long" ? 0 : 1)
      ])) as bigint,
      depth: async (asset) => {
        const raw = await env.KV.get(`depth: ${asset}`);
        if (raw === null) {
          return null;
        }
        const value = Number(raw);
        if (!Number.isFinite(raw) || value < 0) {
          throw new Error(`Invalide depth for ${asset}`);
        }
        return value;
      },
      shock: async (asset) => {
        const raw = await env.KV.get(`shock: ${asset}`);
        if (raw === null) {
          return null
        }
        const value: unknown = JSON.parse(raw);
        if (typeof value !== "object" || value === null
          || !("factor" in value) || !("until" in value)
          || typeof value.factor !== "number" 
          || !Number.isFinite(value.factor) || value.factor <= 0
          || typeof value.until !== "number" || !Number.isSafeInteger(value.until)
        ) {
          throw new Error(`Invalid shock for ${asset}`);
        }
        return value as Shock;
      },
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
    if (action === "open") {
      const assetName = body.asset;
      if (assetName !== "XLM" && assetName!== "BTC" && assetName !== "ETH") {
        throw new BadRequest("unsupported asset");
      }
      if (body.direction !== "long" && body.direction !== "short") {
        throw new BadRequest("direction must be long or short");
      }
      const notional7 = parseUnits(body.notional);
      if (notional7 <= 0n) {
        throw new BadRequest("notional must be positive");
      }
      const config = await read(n, env.VAULT_ID, "config");
      if (typeof config !== "object" || config === null
        || !("oracle" in config) || typeof config.oracle !== "string"
      ) {
        throw new Upstream("oracle address unvailable");
      }
      const oracleAsset = xdr.ScVal.scvVec([
        xdr.ScVal.scvSymbol("Other"),
        xdr.ScVal.scvSymbol(assetName)
      ]);
      const priceData = await read(n, config.oracle, "lastprice", [oracleAsset]);
      if (typeof priceData !== "object" || priceData === null
        || !("price" in priceData) || typeof priceData.price !== "bigint"
      ) {
        throw new Upstream("oracle price unvailable");
      }
      const price7 = priceData.price / 10_000_000n;
      if (price7 <= 0n) {
        throw new Upstream("invalid oracle price");
      }
      const size = (notional7 * 10_000_000n) / price7;
      if (size <= 0n) {
        throw new BadRequest("position size too small");
      }
      const direction = body.direction === "long" ? 0 : 1;
      const tx = await prepareInvoke(n, source, env.PERPDEX_ID, "open", [
        addr(source),
        xdr.ScVal.scvSymbol(assetName),
        xdr.ScVal.scvU32(direction),
        nativeToScVal(size, {type: "i128"})
      ]);
      return json({tx});
    }
    if (action === "close") {
      const args = [
        positionId(body.positionId),
        body.price === undefined || body.price === null
          ? xdr.ScVal.scvVoid()
          : i128(body.price)
      ];

      return json({
        tx: await prepareInvoke(
          n,
          source,
          env.PERPDEX_ID,
          "close",
          args
        )
      });
    }
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


export async function keep(env: Env): Promise<void> {
  const n: Net = {
    rpcUrl: env.RPC_URL,
    passphrase: env.NETWORK_PASSPHRASE
  };

  const safeId = (value: unknown): number => {
    if (typeof value !== "bigint" && typeof value !== "number") {
      throw new Error("Invalid protection ID");
    }

    const id = Number(value);

    if (!Number.isSafeInteger(id) || id < 0) {
      throw new Error("Protection ID is out of range");
    }

    return id;
  };

  await runKeeper({
    now: () => Math.floor(Date.now() / 1000),

    nextId: async () => {
      const value = await read(n, env.VAULT_ID, "next_id");
      return safeId(value);
    },

    cursor: {
      get: async () => {
        const raw = await env.KV.get("keeper:cursor");
        return raw === null ? 0 : safeId(Number(raw));
      },

      set: async (id) => {
        await env.KV.put("keeper:cursor", String(id));
      }
    },

    protection: async (id) => {
      try {
        const result = await read(n, env.VAULT_ID, "protection", [
          nativeToScVal(BigInt(id), { type: "u64" })
        ]);

        return result === undefined || result === null
          ? undefined
          : result as KeptProtection;
      } catch (e) {
        if (e instanceof Rejected && e.name === "NotFound") {
          return undefined;
        }

        throw e;
      }
    },

    position: async (perpdex, id) => {
      try {
        const result = await read(n, perpdex, "get_position", [
          nativeToScVal(id)
        ]);

        return result === undefined || result === null
          ? undefined
          : result as { status: number; size: bigint };
      } catch (e) {
        if (e instanceof Rejected && e.name === "NotFound") {
          return undefined;
        }

        throw e;
      }
    },

    call: async (method, id) => {
      const receipt = await invokeAs(
        n,
        env.KEEPER_SECRET,
        env.VAULT_ID,
        method,
        [nativeToScVal(BigInt(id), { type: "u64" })]
      );

      if (receipt.status !== "SUCCESS") {
        throw new Error(
          `${method} #${id}: ${receipt.status}`
        );
      }

      return receipt.hash;
    },

    log: (msg) => console.error(`[keeper] ${msg}`)
  });
}


export async function watchShock(env: Env): Promise<void> {
  const raw = await env.KV.get("vol-tables");

  const tables = (
    raw === null ? volTables : JSON.parse(raw)
  ) as Record<string, VolTable>;

  const assets = ["XLM", "BTC", "ETH"] as const;

  for (const asset of assets) {
    try {
      // 1. Read the 1-hour volatility bucket
      const bucket = tables[asset]?.buckets["3600"];

      if (!bucket) {
        throw new Error(`Missing volatility bucket for ${asset}`);
      }

      const threshold = 2 * Math.max(bucket.down, bucket.up);

      if (!Number.isFinite(threshold) || threshold <= 0) {
        throw new Error(`Invalid volatility threshold for ${asset}`);
      }

      // 2. Get 61 closes = 60 one-minute returns
      const closes = await recentCloses(asset, 60, 60);

      if (closes.length !== 61) {
        throw new Error(`Not enough price data for ${asset}`);
      }

      // 3. Calculate annualized realized volatility
      const sigma = realizedVol(closes, 60);

      if (!Number.isFinite(sigma)) {
        throw new Error(`Invalid realized volatility for ${asset}`);
      }

      // 4. Detect volatility shock
      if (sigma > threshold) {
        const now = Math.floor(Date.now() / 1000);

        const shock = {
          factor: 2,
          until: now + 900
        };

        await env.KV.put(
          `shock:${asset}`,
          JSON.stringify(shock),
          { expirationTtl: 900 }
        );

        console.warn(
          `[shock] ${asset}: sigma=${sigma}, threshold=${threshold}`
        );
      }
    } catch (error) {
      const message =
        error instanceof Error ? error.message : String(error);

      console.error(`[shock] ${asset} failed: ${message}`);
    }
  }
}

export async function refreshDepth(env: Env): Promise<void> {
  const assets = ["XLM", "BTC", "ETH"] as const;

  for (const asset of assets) {
    try {
      const depth = await depthUsd(asset);

      if (!Number.isFinite(depth) || depth < 0) {
        throw new Error(`Invalid market depth for ${asset}`);
      }

      await env.KV.put(`depth:${asset}`, String(depth));

      console.log(`[depth] ${asset}: ${depth} USD`);
    } catch (error) {
      const message =
        error instanceof Error ? error.message : String(error);

      console.error(`[depth] ${asset} failed: ${message}`);
    }
  }
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
  async scheduled(event: ScheduledController, env: Env, ctx: ExecutionContext): Promise<void> {
    switch (event.cron) {
      case "* * * * *":
        ctx.waitUntil(keep(env));
        ctx.waitUntil(watchShock(env));
        break;
      case "0 0 * * *":
        ctx.waitUntil(refreshDepth(env));
        break;
      default:
        console.warn(`[cron] Unknown schedule: ${event.cron}`);
      }
  }
};

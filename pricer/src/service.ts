import { StrKey } from "@stellar/stellar-sdk";
import { price, type Direction, type Price } from "./pricing.ts";
import { signQuote, type Quote } from "./quote.ts";

export const UNIT = 10_000_000n;
export const QUOTE_TTL_SECS = 30n;
const MAX_AMOUNT = 50_000n * UNIT;

export interface VolTable {
  buckets: Record<string, { down: number; up: number }>;
}

export const ASSETS = ["XLM", "BTC", "ETH"] as const;

export interface Position { 
  owner: string; 
  asset: string; 
  direction: number; 
  size: bigint; 
  mark_price: bigint; 
  status: number 
}

export interface VaultState { 
  capital: bigint; 
  reserved: bigint; 
  locked: bigint 
}

export interface Shock { 
  factor: number; 
  until: number 
}

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

export class BadRequest extends Error {}
export class Upstream extends Error {}

export interface SignedQuote {
  quote: Quote;
  xdr: string;
  signature: string;
  breakdown: Price & { sigma: number; utilization: number; concentration: number };
}

export function parseUnits(s: unknown): bigint {
  if (typeof s !== "string" || !/^\d{1,9}(\.\d{1,7})?$/.test(s)) throw new BadRequest("amount must be a decimal string with up to 7 places");
  const [whole, frac = ""] = s.split(".");
  return BigInt(whole) * UNIT + BigInt(frac.padEnd(7, "0"));
}

export async function makeQuote(input: Record<string, unknown>, d: Deps): Promise<SignedQuote> {
  const { positionId, duration, rangeBps } = input;
  if (typeof positionId !== "string" || !/^[0-9a-f]{64}$/.test(positionId)) {
    throw new BadRequest("Invalid positionId");
  }
  if (!Number.isInteger(duration)) {
    throw new BadRequest("duration must be an interger");
  }

  if (!Number.isInteger(rangeBps) || (rangeBps as number) < 500 || (rangeBps as number) > 5_000) {
    throw new BadRequest("rangeBps must be an integer from 500 to 5000");
  }
  const amount = parseUnits(input.amount);
  if (amount <= 0n || amount > MAX_AMOUNT) throw new BadRequest("amount must be > 0 and <= 50000");

  let pos: Position;
  let vault: VaultState;
  try {
    [pos, vault] = await Promise.all([d.position(positionId), d.vaultState()]);
  } catch (e) {
    throw new Upstream(`position or vault state unavailable: ${(e as Error).message}`);
  }
  if (pos.mark_price <= 0) throw new Upstream("price feed returned an invalid price");
  if (pos.status !== 0) {
    throw new BadRequest("position is not open");
  }

  const bucket = d.volTables[pos.asset]?.buckets[String(duration)];
  if (bucket === undefined) {
    throw new BadRequest(`duration must be one of ${Object.keys(d.volTables[pos.asset]?.buckets?? {}).join(", ")} seconds`);
  }
  if (amount * UNIT > pos.size * pos.mark_price) {
    throw new BadRequest("More cover than the position is worth");
  }
  
  const dir: Direction = pos.direction === 0 ? "long" : "short" ;
  const opposite : Direction = dir === "long" ? "short" : "long";
  let side: bigint;
  let other: bigint;
  let depth: number | null;
  let shock: Shock | null;
  try {
    [side, other, depth, shock] = await Promise.all([
      d.exposure(pos.asset, dir),
      d.exposure(pos.asset, opposite),
      d.depth(pos.asset),
      d.shock(pos.asset),
    ]);
  } catch (e) {
    throw new Upstream(`risk data unavailable: ${(e as Error).message}`);
  }
  
  let sigma = dir === "long" ? bucket.down : bucket.up;
  const now = d.now();
  if (shock !== null && BigInt(shock.until) > now) {
    if (Number(duration) <= 3600) {
      throw new Upstream("...paused...");
    }
    sigma *= shock.factor;
  }

  const r = BigInt(rangeBps as number);
  const maxPayout = (amount * r) / 10_000n;
  if (depth !== null && Number(side + other + maxPayout) / 1e7 > 0.5 * depth) {
    throw new BadRequest(`${pos.asset} liquidity cap reached`);
  }

  const available = vault.capital - vault.locked;
  let utilization = available > 0n ? Number(vault.reserved + maxPayout) / Number(available) : Infinity;
  let concentration = available > 0n ? Number(side + maxPayout) / Number(available) : Infinity;
  
  // ponytail: concentration loading off until per-(asset, direction) reserves land on-chain (Instaward 3)
  const p = price(dir, Number(amount) / 1e7, Number(r) / 10_000, sigma, Number(duration), utilization, concentration);
  
  const quote: Quote = {
    vault: d.vault,
    perpdex: d.perpdex,
    position_id: positionId,
    trader: pos.owner,
    asset: pos.asset,
    direction: dir,
    amount,
    qty: (amount * UNIT) / pos.mark_price,
    p_start: pos.mark_price,
    p_end: dir === "long" ? (pos.mark_price * (10_000n - r)) / 10_000n : (pos.mark_price * (10_000n + r)) / 10_000n,
    duration: BigInt(duration as number),
    max_payout: maxPayout,
    premium: BigInt(Math.ceil(p.premium * 1e7)),
    fee: BigInt(Math.ceil(p.fee * 1e7)),
    vol_bps: Math.round(sigma * 10_000),
    quoted_at: now,
    expires_at: now + QUOTE_TTL_SECS,
    nonce: d.nonce(),
  };
  return { quote, ...signQuote(quote, d.secret), breakdown: { ...p, sigma, utilization, concentration } };
}

export function randomNonce(): bigint {
  const b = new BigUint64Array(1);
  crypto.getRandomValues(b);
  return b[0];
}

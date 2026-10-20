import { StrKey } from "@stellar/stellar-sdk";
import { price, type Direction, type Price } from "./pricing.ts";
import { signQuote, type Quote } from "./quote.ts";

export const UNIT = 10_000_000n;
export const QUOTE_TTL_SECS = 30n;
const MAX_AMOUNT = 50_000n * UNIT;

export interface VolTable {
  buckets: Record<string, { down: number; up: number }>;
}

export interface Deps {
  secret: string;
  vault: string;
  volTable: VolTable;
  now: () => bigint;
  nonce: () => bigint;
  spot: () => Promise<number>;
  vaultState: () => Promise<{ capital: bigint; reserved: bigint }>;
}

export class BadRequest extends Error {}
export class Upstream extends Error {}

export interface SignedQuote {
  quote: Quote;
  xdr: string;
  signature: string;
  breakdown: Price & { sigma: number; utilization: number };
}

export function parseUnits(s: unknown): bigint {
  if (typeof s !== "string" || !/^\d{1,9}(\.\d{1,7})?$/.test(s)) throw new BadRequest("amount must be a decimal string with up to 7 places");
  const [whole, frac = ""] = s.split(".");
  return BigInt(whole) * UNIT + BigInt(frac.padEnd(7, "0"));
}

export async function makeQuote(input: Record<string, unknown>, d: Deps): Promise<SignedQuote> {
  const { trader, direction, duration, rangeBps } = input;
  if (typeof trader !== "string" || !(StrKey.isValidEd25519PublicKey(trader) || StrKey.isValidContract(trader))) {
    throw new BadRequest("trader must be a Stellar address");
  }
  if (direction !== "long" && direction !== "short") throw new BadRequest("direction must be long or short");
  const bucket = d.volTable.buckets[String(duration)];
  if (!bucket) throw new BadRequest(`duration must be one of ${Object.keys(d.volTable.buckets).join(", ")} seconds`);
  if (!Number.isInteger(rangeBps) || (rangeBps as number) < 500 || (rangeBps as number) > 5_000) {
    throw new BadRequest("rangeBps must be an integer from 500 to 5000");
  }
  const amount = parseUnits(input.amount);
  if (amount <= 0n || amount > MAX_AMOUNT) throw new BadRequest("amount must be > 0 and <= 50000");

  let spot: number;
  let vault: { capital: bigint; reserved: bigint };
  try {
    [spot, vault] = await Promise.all([d.spot(), d.vaultState()]);
  } catch (e) {
    throw new Upstream(`price or vault state unavailable: ${(e as Error).message}`);
  }
  if (!Number.isFinite(spot) || spot <= 0) throw new Upstream("price feed returned an invalid price");

  const dir = direction as Direction;
  const r = BigInt(rangeBps as number);
  const pStart = BigInt(Math.round(spot * 1e7));
  const maxPayout = (amount * r) / 10_000n;
  const sigma = dir === "long" ? bucket.down : bucket.up;
  const utilization = vault.capital > 0n ? Number(vault.reserved + maxPayout) / Number(vault.capital) : Infinity;
  // ponytail: concentration loading off until per-(asset, direction) reserves land on-chain (Instaward 3)
  const p = price(dir, Number(amount) / 1e7, Number(r) / 10_000, sigma, Number(duration), utilization, 0);
  const now = d.now();

  const quote: Quote = {
    vault: d.vault,
    trader,
    asset: "XLM",
    direction: dir,
    amount,
    qty: (amount * UNIT) / pStart,
    p_start: pStart,
    p_end: dir === "long" ? (pStart * (10_000n - r)) / 10_000n : (pStart * (10_000n + r)) / 10_000n,
    duration: BigInt(duration as number),
    max_payout: maxPayout,
    premium: BigInt(Math.ceil(p.premium * 1e7)),
    fee: BigInt(Math.ceil(p.fee * 1e7)),
    vol_bps: Math.round(sigma * 10_000),
    quoted_at: now,
    expires_at: now + QUOTE_TTL_SECS,
    nonce: d.nonce(),
  };
  return { quote, ...signQuote(quote, d.secret), breakdown: { ...p, sigma, utilization } };
}

export function randomNonce(): bigint {
  const b = new BigUint64Array(1);
  crypto.getRandomValues(b);
  return b[0];
}

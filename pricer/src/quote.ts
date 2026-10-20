import { Keypair, nativeToScVal, xdr } from "@stellar/stellar-sdk";
import type { Direction } from "./pricing.ts";

export interface Quote {
  vault: string;
  trader: string;
  asset: string;
  direction: Direction;
  amount: bigint;
  qty: bigint;
  p_start: bigint;
  p_end: bigint;
  duration: bigint;
  max_payout: bigint;
  premium: bigint;
  fee: bigint;
  vol_bps: number;
  quoted_at: bigint;
  expires_at: bigint;
  nonce: bigint;
}

const TYPES = {
  vault: "address",
  trader: "address",
  asset: "symbol",
  direction: "u32",
  amount: "i128",
  qty: "i128",
  p_start: "i128",
  p_end: "i128",
  duration: "u64",
  max_payout: "i128",
  premium: "i128",
  fee: "i128",
  vol_bps: "u32",
  quoted_at: "u64",
  expires_at: "u64",
  nonce: "u64",
} as const satisfies Record<keyof Quote, string>;

/** Same bytes as the contract's `Quote.to_xdr()`: a map with keys in sorted order. */
export function quoteToScVal(q: Quote): xdr.ScVal {
  const keys = (Object.keys(TYPES) as (keyof Quote)[]).sort();
  return xdr.ScVal.scvMap(
    keys.map((k) => {
      const v = k === "direction" ? (q.direction === "long" ? 0 : 1) : q[k];
      return new xdr.ScMapEntry({ key: xdr.ScVal.scvSymbol(k), val: nativeToScVal(v, { type: TYPES[k] }) });
    }),
  );
}

export function signQuote(q: Quote, secret: string): { xdr: string; signature: string } {
  const bytes = Buffer.from(quoteToScVal(q).toXDR());
  const signature = Buffer.from(Keypair.fromSecret(secret).sign(bytes));
  return { xdr: bytes.toString("base64"), signature: signature.toString("hex") };
}

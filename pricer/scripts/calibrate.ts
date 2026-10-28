// Builds vol-table.json from Binance XLMUSDT history using quantile-matched σ (spec §3.3).
import { writeFileSync } from "node:fs";
import { BUCKETS, expectedLoss, impliedSigma, PARAMS, YEAR_SECS, type Direction } from "../src/pricing.ts";

const asset = process.argv[2];
if (asset !== "XLM" && asset !== "BTC" && asset !== "ETH") {
  throw new Error("Usage: npm run calibrate -- <XLM|BTC|ETH>")
}
const SYMBOL = `${asset}-USD`;
const API = `https://api.exchange.coinbase.com/products/${SYMBOL}/candles`;
const R_REF = 0.25;
const FLOORS = {
  XLM: 0.6,
  BTC: 0.5,
  ETH: 0.5,
};
const FLOOR = FLOORS[asset];

type Candle = [
  time: number,
  low: number,
  high: number,
  open: number,
  close: number,
  volume: number
];

async function closes(stepSecs: number, days: number): Promise<number[]> {
  const stepMs = stepSecs * 1000;
  const end = Math.floor(Date.now() / stepMs) * stepMs;
  let start = end - days * 86_400_000;
  let cursor = end;
  const byTime = new Map<number, number>();

  while (cursor > start) {
    const pageStart = Math.max(start, cursor - 300 * stepSecs * 1000);
    const url = `${API}?granularity=${stepSecs}` +
      `&start=${new Date(pageStart).toISOString()}` +
      `&end=${new Date(cursor).toISOString()}`;

    const res = await fetch(url, {
      headers: {
        "user-agent": "TOSBI-Pricer",
      },
    });
    if (!res.ok) throw new Error(`Coinbase candles: HTTP ${res.status}`);
    const rows = (await res.json()) as Candle[];
    for (const row of rows) {
      const time = row[0];
      const close = row[4];

      if (time * 1000 >= start && time * 1000 <= end) {
        byTime.set(time, close);
      }
    }

    cursor = pageStart - stepMs;
  }
  return [...byTime.entries()].sort(([a], [b]) => a - b).map(([, close]) => close);
}

const round4 = (x: number) => Math.round(x * 10_000) / 10_000;
const series = { short: await closes(300, 30), long: await closes(3_600, 90) };

const buckets = Object.fromEntries(
  BUCKETS.map((secs) => {
    const [data, step] = secs <= 43_200 ? [series.short, 300] : [series.long, 3_600];
    const h = secs / step;
    const fit = (dir: Direction) =>
      impliedSigma(dir, expectedLoss(dir, data, h, R_REF), secs / YEAR_SECS, R_REF);
    const rawDown = fit("long");
    const rawUp = fit("short");
    const clamp = (s: number) => Math.min(Math.max(s, FLOOR), PARAMS.sigmaCap);
    return [
      secs,
      { down: round4(clamp(rawDown)), up: round4(clamp(rawUp)), raw_down: round4(rawDown), raw_up: round4(rawUp), samples: data.length - h },
    ];
  }),
);

const table = {
  asset: asset,
  source: `Binance ${SYMBOL} closes: 30d of 5m for buckets <= 12h, 90d of 1h for >= 1d`,
  generated_at: new Date().toISOString(),
  r_ref: R_REF,
  floor: FLOOR,
  cap: PARAMS.sigmaCap,
  buckets,
};
writeFileSync(
  new URL(`../vol-tables/${asset}.json`, import.meta.url),
  JSON.stringify(table, null, 2) + "\n"
);
console.table(buckets);

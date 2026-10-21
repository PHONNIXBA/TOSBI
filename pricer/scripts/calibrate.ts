// Builds vol-table.json from Binance XLMUSDT history using quantile-matched σ (spec §3.3).
import { writeFileSync } from "node:fs";
import { BUCKETS, expectedLoss, impliedSigma, PARAMS, YEAR_SECS, type Direction } from "../src/pricing.ts";

const SYMBOL = "XLMUSDT";
const API = "https://data-api.binance.vision/api/v3/klines";
const R_REF = 0.25;
const FLOOR = 0.6;

async function closes(interval: string, stepSecs: number, days: number): Promise<number[]> {
  const out: number[] = [];
  const end = Date.now();
  let start = end - days * 86_400_000;
  while (start < end) {
    const res = await fetch(`${API}?symbol=${SYMBOL}&interval=${interval}&startTime=${start}&limit=1000`);
    if (!res.ok) throw new Error(`klines ${interval}: HTTP ${res.status}`);
    const rows = (await res.json()) as [number, string, string, string, string][];
    if (rows.length === 0) break;
    out.push(...rows.map((r) => Number(r[4])));
    start = rows[rows.length - 1][0] + stepSecs * 1000;
  }
  return out;
}

const round4 = (x: number) => Math.round(x * 10_000) / 10_000;
const series = { short: await closes("5m", 300, 30), long: await closes("1h", 3_600, 90) };

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
  asset: "XLM",
  source: `Binance ${SYMBOL} closes: 30d of 5m for buckets <= 12h, 90d of 1h for >= 1d`,
  generated_at: new Date().toISOString(),
  r_ref: R_REF,
  floor: FLOOR,
  cap: PARAMS.sigmaCap,
  buckets,
};
writeFileSync(new URL("../vol-table.json", import.meta.url), JSON.stringify(table, null, 2) + "\n");
console.table(buckets);

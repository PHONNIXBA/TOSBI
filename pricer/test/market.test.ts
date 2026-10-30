import { test } from "node:test";
import assert from "node:assert/strict";
import { bookDepth, realizedVol } from "../src/market.ts";
import { YEAR_SECS } from "../src/pricing.ts";

test("realizedVol annualizes the stdev of log returns", () => {
  assert.equal(realizedVol([1, 1, 1, 1], 60), 0);
  // returns +r, −r, +r, −r: stdev = r·√(4/3)
  const r = 0.01;
  const closes = [1, Math.exp(r), 1, Math.exp(r), 1];
  const expected = r * Math.sqrt(4 / 3) * Math.sqrt(YEAR_SECS / 60);
  assert.ok(Math.abs(realizedVol(closes, 60) - expected) < 1e-9);
});

test("bookDepth sums notional out to the move and stops past it", () => {
  const bids: [number, number][] = [[100, 1], [90, 2], [76, 5], [74, 100]];
  assert.equal(bookDepth(bids, 0.25, "bids"), 100 + 180 + 380);
  const asks: [number, number][] = [[100, 1], [125, 2], [126, 100]];
  assert.equal(bookDepth(asks, 0.25, "asks"), 100 + 250);
  assert.equal(bookDepth([], 0.25, "asks"), 0);
});

import { test } from "node:test";
import assert from "node:assert/strict";
import { expectedLoss, fairFraction, impliedSigma, normCdf, price, YEAR_SECS } from "../src/pricing.ts";

const close = (a: number, b: number, tol: number) => assert.ok(Math.abs(a - b) <= tol, `${a} vs ${b}`);
const DAY = 86_400;

test("normCdf matches reference values including tails", () => {
  close(normCdf(0), 0.5, 1e-7);
  close(normCdf(1.96), 0.9750021, 1e-7);
  close(normCdf(-5) / 2.866516e-7, 1, 0.001);
});

test("H2: LONG reproduces spec §10.1 (σ_down 100%, 1d, R 25%, U' 0.5, C' 0.2)", () => {
  const p = price("long", 1_000, 0.25, 1.0, DAY, 0.5, 0.2);
  close(p.fair, 20.87, 0.01);
  close(p.mU, 1.125, 1e-12);
  close(p.mC, 1.04, 1e-12);
  close(p.premium, 25.64, 0.01);
  close(p.fee, 3.0, 1e-12);
});

test("H3: SHORT prices a call spread from σ_up and reproduces spec §10.2", () => {
  const p = price("short", 1_000, 0.25, 0.9, DAY, 0.5, 0.1);
  close(p.fair, 18.79, 0.01);
  close(p.premium, 22.41, 0.01);
});

test("E8: premium and fee floors apply to tiny, short, deep covers", () => {
  const p = price("long", 20, 0.05, 0.6, 900, 0, 0);
  assert.equal(p.premium, 0.5);
  assert.equal(p.fee, 0.5);
  assert.equal(price("long", 50_000, 0.05, 0.4, 900, 0, 0).premium, 50);
});

test("E10: utilization above 1 still prices (contract enforces capacity)", () => {
  const p = price("long", 1_000, 0.25, 1.0, DAY, 7.5, 0);
  assert.ok(Number.isFinite(p.premium));
  close(p.mU, 1.5, 1e-12);
});

test("impliedSigma inverts fairFraction for every bucket and direction", () => {
  for (const secs of [900, 3_600, 14_400, 43_200, DAY, 3 * DAY, 7 * DAY]) {
    for (const dir of ["long", "short"] as const) {
      for (const sigma of [0.3, 0.8, 2.5]) {
        const tau = secs / YEAR_SECS;
        close(impliedSigma(dir, fairFraction(dir, sigma, tau, 0.25), tau, 0.25), sigma, 1e-6);
      }
    }
  }
});

test("E9: impliedSigma clamps to [0.01, cap]", () => {
  const tau = DAY / YEAR_SECS;
  assert.equal(impliedSigma("long", 0, tau, 0.25), 0.01);
  assert.equal(impliedSigma("long", 0.25, tau, 0.25), 3.0);
});

test("expectedLoss caps losses at the range and splits falls from rises", () => {
  const closes = [1, 0.5, 0.5, 1.5];
  assert.equal(expectedLoss("long", closes, 1, 0.25), (0.25 + 0 + 0) / 3);
  assert.equal(expectedLoss("short", closes, 1, 0.25), (0 + 0 + 0.25) / 3);
});

export type Direction = "long" | "short";

export const YEAR_SECS = 31_557_600;
export const BUCKETS = [900, 3_600, 14_400, 43_200, 86_400, 259_200, 604_800] as const;

export const PARAMS = {
  lambdaU: 0.5,
  pU: 2,
  lambdaC: 1.0,
  pC: 2,
  rho: 0.05,
  premiumMinUsdc: 0.5,
  premiumMinFrac: 0.001,
  feeMinUsdc: 0.5,
  feeFrac: 0.003,
  sigmaCap: 3.0,
};

// Numerical Recipes erfcc: fractional error < 1.2e-7 everywhere, so tails stay accurate.
function erfc(x: number): number {
  const z = Math.abs(x);
  const t = 1 / (1 + 0.5 * z);
  const r =
    t *
    Math.exp(
      -z * z - 1.26551223 +
        t * (1.00002368 + t * (0.37409196 + t * (0.09678418 + t * (-0.18628806 +
        t * (0.27886807 + t * (-1.13520398 + t * (1.48851587 + t * (-0.82215223 + t * 0.17087277)))))))),
    );
  return x >= 0 ? r : 2 - r;
}

export const normCdf = (x: number): number => 0.5 * erfc(-x / Math.SQRT2);

function d1d2(s: number, k: number, sigma: number, tau: number): [number, number] {
  const v = sigma * Math.sqrt(tau);
  const d1 = (Math.log(s / k) + (v * v) / 2) / v;
  return [d1, d1 - v];
}

export function bsPut(s: number, k: number, sigma: number, tau: number): number {
  const [d1, d2] = d1d2(s, k, sigma, tau);
  return k * normCdf(-d2) - s * normCdf(-d1);
}

export function bsCall(s: number, k: number, sigma: number, tau: number): number {
  const [d1, d2] = d1d2(s, k, sigma, tau);
  return s * normCdf(d1) - k * normCdf(d2);
}

/** Fair spread value as a fraction of the protected amount (spec §3.2, r = 0). */
export function fairFraction(dir: Direction, sigma: number, tau: number, range: number): number {
  return dir === "long"
    ? bsPut(1, 1, sigma, tau) - bsPut(1, 1 - range, sigma, tau)
    : bsCall(1, 1, sigma, tau) - bsCall(1, 1 + range, sigma, tau);
}

/**
 * σ whose spread price equals `target` (spec §3.3). The spread is increasing in σ on [0.01, cap]
 * for every bucket ≤ 7d at R ≤ 0.5, so bisection is safe; targets outside the range clamp.
 */
export function impliedSigma(dir: Direction, target: number, tau: number, range: number): number {
  let lo = 0.01;
  let hi = PARAMS.sigmaCap;
  if (target <= fairFraction(dir, lo, tau, range)) return lo;
  if (target >= fairFraction(dir, hi, tau, range)) return hi;
  for (let i = 0; i < 100; i++) {
    const mid = (lo + hi) / 2;
    if (fairFraction(dir, mid, tau, range) < target) lo = mid;
    else hi = mid;
  }
  return (lo + hi) / 2;
}

/** Empirical expected capped loss fraction over rolling `h`-step windows of `closes`. */
export function expectedLoss(dir: Direction, closes: number[], h: number, range: number): number {
  let sum = 0;
  const n = closes.length - h;
  for (let i = 0; i < n; i++) {
    const g = closes[i + h] / closes[i];
    const loss = dir === "long" ? 1 - g : g - 1;
    sum += Math.max(0, Math.min(range, loss));
  }
  return sum / n;
}

export interface Price {
  fair: number;
  mU: number;
  mC: number;
  premium: number;
  fee: number;
}

/** Charged premium and fee in USDC (spec §3.5–3.7). `utilization`/`concentration` include the new contract. */
export function price(
  dir: Direction,
  amount: number,
  range: number,
  sigma: number,
  durationSecs: number,
  utilization: number,
  concentration: number,
): Price {
  const p = PARAMS;
  const fair = amount * fairFraction(dir, sigma, durationSecs / YEAR_SECS, range);
  const mU = 1 + p.lambdaU * Math.min(utilization, 1) ** p.pU;
  const mC = 1 + p.lambdaC * Math.min(concentration, 1) ** p.pC;
  const loaded = fair * mU * mC * (1 + p.rho);
  const premium = Math.max(loaded, p.premiumMinUsdc, p.premiumMinFrac * amount);
  const fee = Math.max(p.feeMinUsdc, p.feeFrac * amount);
  return { fair, mU, mC, premium, fee };
}

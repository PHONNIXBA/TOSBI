import { YEAR_SECS } from "./pricing.ts";

// annualized, sample stdev of log returns
export function realizedVol(closes: number[], stepSecs: number): number {
    if (closes.length < 3 || stepSecs <= 0) {
        throw new Error("Invalide input for realized volatility");
    }

    const returns: number[] = [];

    for (let i = 0; i < closes.length - 1; i++) {
        const currentPrice = closes[i];
        const nextPrice = closes[i + 1];

        if (currentPrice <= 0 || nextPrice <= 0) {
            throw new Error("Closing prices must be positive");
        }

        const logReturn = Math.log(nextPrice / currentPrice);
        returns.push(logReturn);
    }

    const n = returns.length;
    const mean = returns.reduce((sum, value) => sum + value, 0) / n;
    const squareDeviations = returns.reduce((sum, value) => sum + (value - mean) ** 2, 0);
    const sampleVariance = squareDeviations / (n - 1);
    const annualizedVol = Math.sqrt(sampleVariance * (YEAR_SECS / stepSecs));
    return annualizedVol;
} 

export function bookDepth(levels: [number, number][], move: number, side: "bids" | "asks"): number {
    if (levels.length === 0) {
        return 0;
    }

    const bestPrice = levels[0][0];
    const limit = side === "bids" ? bestPrice * (1 - move) : bestPrice * (1 + move);
    let totalDepth = 0;

    for (const [price, size] of levels) {
        const outsideLimit = side === "bids" ? price < limit : price > limit;

        if (outsideLimit) {
            break;
        }
        totalDepth += price * size;
    }
    return totalDepth;
}

// oldest first
export async function recentCloses(asset: string, stepSecs: number, count: number): Promise<number[]> {
    const product = `${asset.toUpperCase()}-USD`;
    const url = new URL(`https://api.exchange.coinbase.com/products/${product}/candles`);
    url.searchParams.set("granularity", String(stepSecs));

    const response = await fetch(url, {
        headers: {
            "User-Agent": "TOSBI-Market-Data",
        },
    });

    if (!response.ok) {
        throw new Error(`Coinbase returned HTTP ${response.status}`);
    }
    
    const candles = (await response.json()) as number[][];

    const closes = candles.slice(0, count + 1).reverse().map(candle => candle[4]);
    return closes;
}

// Coinbase + Kraken, thinner side, 25% move
export async function depthUsd(asset: string): Promise<number> {
    const symbols: Record<string, string> = {
        XLM: "XLMUSD",
        BTC: "BTCUSD",
        ETH: "ETHUSD",
    };

    const name = asset.toUpperCase();
    const krakenPair = symbols[name];
    if (!krakenPair) {
        throw new Error(`Unsupported asset: ${asset}`);
    }

    const coinbaseUrl = `https://api.exchange.coinbase.com/products/${name}-USD/book?level=2`;
    const krankenUrl = `https://api.kraken.com/0/public/Depth?pair=${krakenPair}&count=500`;
    const [coinbaseResponse, krakenResponse] = await Promise.all([
        fetch(coinbaseUrl),
        fetch(krankenUrl),
    ]);
    if (!coinbaseResponse.ok || !krakenResponse.ok) {
        throw new Error("Failed to fetch order books");
    }

    const coinbase = await coinbaseResponse.json() as {
        bids: [string, string][];
        asks: [string, string][];
    };

    const krakenData = await krakenResponse.json() as {
        error: string[];
        result: Record<string, {
        bids: [string, string, number][];
        asks: [string, string, number][];
        }>;
    };

    if (krakenData.error.length > 0) {
        throw new Error(`Kraken error: ${krakenData.error.join(", ")}`);
    }

    const kraken = Object.values(krakenData.result)[0];

    if (!kraken) {
        throw new Error("Kraken order book is missing");
    }

    const toLevels = (
        rows: Array<[string, string, ...unknown[]]>
    ): [number, number][] =>
        rows.map(([price, size]) => [
        Number(price),
        Number(size),
        ]);

    const move = 0.25;

    const bids =
        bookDepth(toLevels(coinbase.bids), move, "bids") +
        bookDepth(toLevels(kraken.bids), move, "bids");

    const asks =
        bookDepth(toLevels(coinbase.asks), move, "asks") +
        bookDepth(toLevels(kraken.asks), move, "asks");

    return Math.min(bids, asks);
}
export type Direction = "LONG" | "SHORT";

export type PayoutInput = {
    direction: Direction,
    protectedAmount: number,
    startPrice: number,
    coverageRange: number,
    settlementPrice: number
};

export type PayoutResult = {
    quantity: number,
    coverageEnd: number,
    maximumPayout: number,
    payout: number
};

export function calculatePayout(input: PayoutInput): PayoutResult {
    const {
        direction,
        protectedAmount,
        startPrice,
        coverageRange,
        settlementPrice
    } = input;

    // TODO 1: tính quantity
    const quantity = protectedAmount / startPrice;

    // TODO 2: tính coverageEnd
    // LONG và SHORT khác nhau
    let coverageEnd: number;
    if (direction === "LONG") {
        coverageEnd = startPrice * (1 - coverageRange);
    } else {
        coverageEnd = startPrice * (1 + coverageRange);
    }


    // TODO 3: tính maximumPayout
    const maximumPayout = protectedAmount * coverageRange;


    // TODO 4: tính loss dựa theo LONG / SHORT
    let payout: number;
    if (direction === "LONG") {
        payout = Math.min(quantity * Math.max(0, startPrice - settlementPrice), maximumPayout);
    } else {
        payout = Math.min(quantity * Math.max(0, settlementPrice - startPrice), maximumPayout);
    }

        // Payout is bounded between 0 and maximumPayout

    return {
        quantity: quantity,
        coverageEnd: coverageEnd,
        maximumPayout: maximumPayout,
        payout: payout
    }
}
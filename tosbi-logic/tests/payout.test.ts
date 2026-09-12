import {describe, it, expect} from "vitest";
import {calculatePayout} from "../src/payout";

describe("calculatePayout", () => {
    it("calculate LONG payout inside coverage range", () => {
        const result = calculatePayout({
            direction: "LONG",
            protectedAmount: 1000,
            startPrice: 0.20,
            coverageRange: 0.25,
            settlementPrice: 0.17
        });

        expect(result.quantity).toBe(5000);
        expect(result.coverageEnd).toBeCloseTo(0.15);
        expect(result.maximumPayout).toBe(250);
        expect(result.payout).toBeCloseTo(150);
    });

    it("returns zero payout when LONG position is profitable", () => {
        const result = calculatePayout({
            direction: "LONG",
            protectedAmount: 1000,
            startPrice: 0.20,
            coverageRange: 0.25,
            settlementPrice: 0.25
        });
        expect(result.quantity).toBe(5000);
        expect(result.coverageEnd).toBeCloseTo(0.15);
        expect(result.maximumPayout).toBe(250);
        expect(result.payout).toBeCloseTo(0);
    });

    it("caps LONG payout at maximumPayout", () => {
        const result = calculatePayout({
            direction: "LONG",
            protectedAmount: 1000,
            startPrice: 0.20,
            coverageRange: 0.25,
            settlementPrice: 0.12
        });
        expect(result.quantity).toBe(5000);
        expect(result.coverageEnd).toBeCloseTo(0.15);
        expect(result.maximumPayout).toBe(250);
        expect(result.payout).toBeCloseTo(250);
    });

    it("returns zero payout when LONG settles exactly at startPrice", () => {
        const result = calculatePayout({
            direction: "LONG",
            protectedAmount: 1000,
            startPrice: 0.20,
            coverageRange: 0.25,
            settlementPrice: 0.20
        });
        expect(result.payout).toBe(0);
    });

    it("calculate SHORT payout inside coverage range", () => {
        const result = calculatePayout({
            direction: "SHORT",
            protectedAmount: 1000,
            startPrice: 0.20,
            coverageRange: 0.25,
            settlementPrice: 0.22 
        });

        expect(result.quantity).toBe(5000);
        expect(result.coverageEnd).toBeCloseTo(0.25);
        expect(result.maximumPayout).toBe(250);
        expect(result.payout).toBeCloseTo(100);
    });

    it("returns zero payout when SHORT position is profitable", () => {
        const result = calculatePayout({
            direction: "SHORT",
            protectedAmount: 1000,
            startPrice: 0.20,
            coverageRange: 0.25,
            settlementPrice: 0.12
        });
        expect(result.quantity).toBe(5000);
        expect(result.coverageEnd).toBeCloseTo(0.25);
        expect(result.maximumPayout).toBe(250);
        expect(result.payout).toBeCloseTo(0);
    });

    it("caps SHORT payout at maximumPayout", () => {
        const result = calculatePayout({
            direction: "SHORT",
            protectedAmount: 1000,
            startPrice: 0.20,
            coverageRange: 0.25,
            settlementPrice: 0.28
        });
        expect(result.quantity).toBe(5000);
        expect(result.coverageEnd).toBeCloseTo(0.25);
        expect(result.maximumPayout).toBe(250);
        expect(result.payout).toBeCloseTo(250);
    });

    it("returns maximumPayout when LONG settles exactly at coverageEnd", () => {
        const result = calculatePayout({
            direction: "LONG",
            protectedAmount: 1000,
            startPrice: 0.20,
            coverageRange: 0.25,
            settlementPrice: 0.15
        });
        expect(result.payout).toBeCloseTo(250);
    });

});
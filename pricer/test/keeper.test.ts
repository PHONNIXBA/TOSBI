import { test } from "node:test";
import assert from "node:assert/strict";
import { runKeeper, type KeeperDeps, type KeptProtection } from "../src/keeper.ts";

const prot = (over: Partial<KeptProtection> = {}): KeptProtection => ({
  status: 0,
  t_expire: 2_000n,
  perpdex: "C_DEX",
  position_id: new Uint8Array(32),
  qty: 100n,
  ...over,
});

function harness(prots: (KeptProtection | undefined)[], positions: Record<number, { status: number; size: bigint }> = {}) {
  const calls: string[] = [];
  let cursor = 0;
  const d: KeeperDeps = {
    now: () => 1_500,
    nextId: async () => prots.length,
    cursor: { get: async () => cursor, set: async (n) => void (cursor = n) },
    protection: async (id) => prots[id],
    position: async (_dex, pid) => positions[pid[31]] ?? { status: 0, size: 100n },
    call: async (method, id) => {
      calls.push(`${method} ${id}`);
      if (id === 99) throw new Error("boom");
      return "SUCCESS";
    },
    log: () => {},
  };
  return { d, calls, cursor: () => cursor };
}

test("settles expired protections and early-settles closed or reduced positions", async () => {
  const pid = (n: number) => Object.assign(new Uint8Array(32), { [31]: n });
  const h = harness(
    [prot({ t_expire: 1_000n }), prot({ position_id: pid(1) }), prot({ position_id: pid(2) }), prot()],
    { 1: { status: 1, size: 100n }, 2: { status: 0, size: 99n } },
  );
  await runKeeper(h.d);
  assert.deepEqual(h.calls, ["settle 0", "settle_early 1", "settle_early 2"]);
});

test("cursor skips the settled prefix and stops at the first active id", async () => {
  const h = harness([prot({ status: 1 }), undefined, prot(), prot({ status: 1 })]);
  await runKeeper(h.d);
  assert.equal(h.cursor(), 2);
  assert.deepEqual(h.calls, []);
});

test("one failed call does not stop the rest", async () => {
  const prots = Array.from({ length: 101 }, () => prot({ status: 1 }));
  prots[99] = prot({ t_expire: 1_000n });
  prots[100] = prot({ t_expire: 1_000n });
  const h = harness(prots);
  await runKeeper(h.d);
  assert.deepEqual(h.calls, ["settle 99", "settle 100"]);
});

test("a failed position read is logged and the next protection is still handled", async () => {
  const h = harness([prot(), prot({ t_expire: 1_000n })]);
  const logs: string[] = [];
  h.d.position = async () => {
    throw new Error("rpc down");
  };
  h.d.log = (m) => logs.push(m);
  await runKeeper(h.d);
  assert.deepEqual(h.calls, ["settle 1"]);
  assert.ok(logs.some((m) => m.includes("#0 failed: rpc down")));
});

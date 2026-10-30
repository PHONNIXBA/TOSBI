export interface KeptProtection { status: number; t_expire: bigint; perpdex: string; position_id: Uint8Array; qty: bigint }
export interface KeeperDeps {
  now: () => number;
  nextId: () => Promise<number>;
  cursor: { get: () => Promise<number>; set: (id: number) => Promise<void> };  // first id that may still be active
  protection: (id: number) => Promise<KeptProtection | undefined>;
  position: (perpdex: string, id: Uint8Array) => Promise<{ status: number; size: bigint } | undefined>;
  call: (method: "settle" | "settle_early", id: number) => Promise<string>;
  log: (msg: string) => void;
}
export async function runKeeper(d: KeeperDeps): Promise<void> {
    const startCursor = await d.cursor.get();
    const nextId = await d.nextId();
    let newCursor = startCursor;
    for (let i = startCursor; i < nextId; i ++) {
        try {
            const protection = await d.protection(i);
            if (!protection || protection.status !== 0) {
                if (newCursor === i) {
                    newCursor = i + 1;
                }
                continue;
            }

            const now = d.now();
            if (BigInt(now) >= protection.t_expire) {
                await d.call("settle", i);
            } else {
                const position = await d.position(protection.perpdex, protection.position_id);
                if (position && (position.status !== 0 || position.size < protection.qty)) {
                    await d.call("settle_early", i);
                }
            }
        } catch (error) {
            const message = error instanceof Error ? error.message : String(error);
            d.log(`#${i} failed: ${message}`);
        }
    }
    if (newCursor !== startCursor) {
        await d.cursor.set(newCursor);
    }
}
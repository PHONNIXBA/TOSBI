import { readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { xdr } from "@stellar/stellar-sdk";
import { invokeAs } from "../src/chain.ts";

const XLM = JSON.parse(readFileSync(
    new URL("../vol-tables/XLM.json", import.meta.url),
    "utf8"
));

const BTC = JSON.parse(readFileSync(
    new URL("../vol-tables/BTC.json", import.meta.url),
    "utf8"
));

const ETH = JSON.parse(readFileSync(
    new URL("../vol-tables/ETH.json", import.meta.url),
    "utf8"
));

const bundle = JSON.stringify({XLM, BTC, ETH});

writeFileSync(
    new URL("../vol-bundle.json", import.meta.url),
    bundle
);

const hash = createHash("sha256").update(bundle).digest("hex");
console.log(hash);

const post = process.argv.includes("--post");
if (post) {
    const {OPERATOR_SECRET, RPC_URL, NETWORK_PASSPHRASE, VAULT_ID} = process.env;
    if (!OPERATOR_SECRET || !RPC_URL || !NETWORK_PASSPHRASE || !VAULT_ID) {
        throw new Error("Missing OPERATOR_SECRET, RPC_URL, NETWORK_PASSPHRASE or VAULT_ID");
    }

    const hashBytes = Buffer.from(hash, "hex");
    const hashArg = xdr.ScVal.scvBytes(hashBytes);
    const receipt = await invokeAs(
        {
            rpcUrl: RPC_URL,
            passphrase: NETWORK_PASSPHRASE,
        },
        OPERATOR_SECRET,
        VAULT_ID,
        "set_vol_hash",
        [hashArg]
    );

    console.log("set_vol_hash receipt:", receipt);
}
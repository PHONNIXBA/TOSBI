# TOSBI — Instaward 1

Protection Vault + Book contract, signed-quote pricer, and testnet demo for [SOW 1](docs/tosbi-instaward-1-sow.md). Design reference: [Architecture 1 spec](docs/tosbi-architecture-1-spec.md).

| SOW deliverable | Where |
|---|---|
| 1. Vault + Book Soroban contract | [contracts/protection/src/lib.rs](contracts/protection/src/lib.rs) |
| 2. Demo page | [pricer/public/index.html](pricer/public/index.html), served with its API by [pricer/src/worker.ts](pricer/src/worker.ts) (Cloudflare Worker) |
| 3. Pricer bootstrap | [pricer/src/pricing.ts](pricer/src/pricing.ts) (Black–Scholes spreads, loadings), [pricer/src/quote.ts](pricer/src/quote.ts) (ed25519 quote signing), [pricer/scripts/calibrate.ts](pricer/scripts/calibrate.ts) → [pricer/vol-table.json](pricer/vol-table.json) |
| 4. Tests | [contracts/protection/src/test.rs](contracts/protection/src/test.rs), [pricer/test/](pricer/test/), [pricer/scripts/e2e.ts](pricer/scripts/e2e.ts) |

## Testnet deployment

| | |
|---|---|
| Demo | https://tosbi-demo.dungnd8594.workers.dev |
| Vault contract | [`CAZXBVORB2QN7V2FHYJFALYDRLBWZGUQDW2GSF3IKGYWFMEVVMZDS4PG`](https://stellar.expert/explorer/testnet/contract/CAZXBVORB2QN7V2FHYJFALYDRLBWZGUQDW2GSF3IKGYWFMEVVMZDS4PG) |
| Deploy tx | [`0b9d31b1…`](https://stellar.expert/explorer/testnet/tx/0b9d31b109e8161435c228442cb982a026bf47f2eea37c11fe419ba876d820fa) (WASM upload [`786678bb…`](https://stellar.expert/explorer/testnet/tx/786678bbd05178254daa2409035f056bf5ba6d55187a950a80cee580b502d4c9), pool deposit [`3f10f50c…`](https://stellar.expert/explorer/testnet/tx/3f10f50ce998af2f119c0aa84cb0c90eb606e017dbf2afa2aa00639a3ec3fc33)) |
| Test token | `tUSDC:GCYYOX6T6DSACBQRPFR57HPLCUSRHSBD5QSQ3PWUCEOQ5OVKXPGBATME` (SAC `CD67MJF6JKMWBEPLB7HETWC7OWIHJMA4FMFEMUJYDUR5J4QZMP54YEUZ`) |
| Pricer public key | `GBHNZIX5RS4QIFAJ2XCMHLS4PKBXQD6QHWLPNA6LG3NH6KVSFMRT7IU4` (raw `4edca2fd8cb9041409d5c4c3ae5c7a83780fd03d96f683cb36da7f2ab22b233f`) |
| Admin | `GDWUXAO43WEFCTXE2FB2GOI2ODEPGB3RDSVWVG6EKMEMBSB4KGWXZWSM` |

Pool seeded with 10,000 tUSDC; per-wallet cap 2% of the pool. The demo's spot price comes from Coinbase, then Kraken, then Binance (Binance blocks Cloudflare).

## Run

```sh
cargo test                      # contract: 27 tests
cd pricer && npm install
npm test && npm run typecheck   # pricer: 16 tests
npm run calibrate               # rebuild vol-table.json from Binance XLMUSDT history

../scripts/deploy.sh local      # or testnet: keys, tUSDC asset, vault, 10,000 tUSDC pool, writes .dev.vars
node scripts/e2e.ts             # drives the Worker API end to end against that deployment
npx wrangler dev                # demo at http://localhost:8787
```

`scripts/deploy.sh local` needs `stellar container start local --protocol-version 29` (Docker). Deploying the demo: copy `VAULT_ID` and `ASSET_ISSUER` from the deploy output into `pricer/wrangler.jsonc`, then `npx wrangler secret put PRICER_SECRET`, `npx wrangler secret put FAUCET_SECRET`, `npx wrangler deploy`.

## Where this differs from the spec

- **Signature** covers the quote's canonical XDR (the contract re-serializes the struct it decoded), not `sha256(Quote)`. Ed25519 hashes internally; re-serializing means extra or missing map keys can't change what was signed.
- **Quote carries `vault`**, so a quote signed for one deployment can't be replayed on another that shares the pricer key. `perpdex` / `position_id` arrive with position binding in Instaward 3.
- **`release`** frees a reservation with no payout. It is the stand-in for settlement until Instaward 2; callable by the admin or the protection's owner.
- **Withdrawals**: `withdraw_request` is limited to free capital and one request at a time, so `reserved ≤ capital − locked` holds at every step; `withdraw_execute` pays out after 7 days.
- **Concentration loading** is 1.0 until per-(asset, direction) reserves are tracked on-chain (Instaward 3). Utilization loading and the 5% margin apply.
- **Prices** use 7 decimals. Reflector uses 14; the Instaward 2 oracle adapter must rescale.
- **Test token** is `tUSDC`, a classic asset issued by the deploy script, with a faucet in the demo.

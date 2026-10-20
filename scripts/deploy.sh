#!/usr/bin/env bash
# Deploys the vault + tUSDC test asset, seeds the pool, and writes pricer/.dev.vars.
# Usage: scripts/deploy.sh [local|testnet]
set -euo pipefail
NET=${1:-testnet}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
CODE=tUSDC
U=10000000
POOL=$((10000 * U))

case "$NET" in
  local) RPC=http://localhost:8000/rpc; PASS="Standalone Network ; February 2017" ;;
  testnet) RPC=https://soroban-testnet.stellar.org; PASS="Test SDF Network ; September 2015" ;;
  *) echo "usage: $0 [local|testnet]" >&2; exit 1 ;;
esac

for k in admin issuer treasury pricer; do
  stellar keys address "tosbi-$k" >/dev/null 2>&1 || stellar keys generate "tosbi-$k" >/dev/null
done
for k in admin issuer treasury; do stellar keys fund "tosbi-$k" --network "$NET" >/dev/null 2>&1 || true; done
addr() { stellar keys address "tosbi-$1"; }
ADMIN=$(addr admin); ISSUER=$(addr issuer); TREASURY=$(addr treasury); PRICER=$(addr pricer)
ASSET="$CODE:$ISSUER"

for k in admin treasury; do
  stellar tx new change-trust --source "tosbi-$k" --line "$ASSET" --network "$NET" >/dev/null
done
stellar contract asset deploy --asset "$ASSET" --source tosbi-admin --network "$NET" >/dev/null 2>&1 || true
TOKEN=$(stellar contract id asset --asset "$ASSET" --network "$NET")
PRICER_HEX=$(cd "$ROOT/pricer" && node --input-type=module -e \
  "import {StrKey} from '@stellar/stellar-sdk'; console.log(Buffer.from(StrKey.decodeEd25519PublicKey('$PRICER')).toString('hex'))")

(cd "$ROOT" && stellar contract build >/dev/null)
PARAMS="{\"a_min\":\"$((20 * U))\",\"a_max\":\"$((50000 * U))\",\"wallet_cap_bps\":200,\"wallet_cap_max\":\"$((25000 * U))\",\"activation_delay\":300,\"withdraw_delay\":604800}"
VAULT=$(stellar contract deploy --wasm "$ROOT/target/wasm32v1-none/release/tosbi_protection.wasm" \
  --source tosbi-admin --network "$NET" --alias tosbi-vault -- \
  --admin "$ADMIN" --token "$TOKEN" --treasury "$TREASURY" --pricer "$PRICER_HEX" --params "$PARAMS")

stellar tx new payment --source tosbi-issuer --destination "$ADMIN" --asset "$ASSET" --amount "$POOL" --network "$NET" >/dev/null
stellar contract invoke --id "$VAULT" --source tosbi-admin --network "$NET" -- deposit_capital --from "$ADMIN" --amount "$POOL" >/dev/null

cat > "$ROOT/pricer/.dev.vars" <<VARS
RPC_URL=$RPC
NETWORK_PASSPHRASE=$PASS
VAULT_ID=$VAULT
ASSET_CODE=$CODE
ASSET_ISSUER=$ISSUER
PRICER_SECRET=$(stellar keys secret tosbi-pricer)
FAUCET_SECRET=$(stellar keys secret tosbi-issuer)
VARS

echo "network     $NET"
echo "vault       $VAULT"
echo "token       $TOKEN ($ASSET)"
echo "admin       $ADMIN"
echo "treasury    $TREASURY"
echo "pricer key  $PRICER ($PRICER_HEX)"
echo "wrote pricer/.dev.vars"

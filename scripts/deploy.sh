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
local)
  RPC=http://localhost:8000/rpc
  PASS="Standalone Network ; February 2017"
  ;;
testnet)
  RPC=https://soroban-testnet.stellar.org
  PASS="Test SDF Network ; September 2015"
  ;;
*)
  echo "usage: $0 [local|testnet]" >&2
  exit 1
  ;;
esac

for k in owner operator keeper issuer treasury pricer signer-tosbi signer-pdx; do
  stellar keys address "tosbi-$k" >/dev/null 2>&1 || stellar keys generate "tosbi-$k" >/dev/null
done

ADMIN_NAME="tosbi-admin-$(date +%s)"
stellar keys generate "$ADMIN_NAME" >/dev/null

for k in owner operator keeper issuer treasury pricer signer-tosbi signer-pdx; do
  stellar keys fund "tosbi-$k" --network "$NET"
done

stellar keys fund "$ADMIN_NAME" --network "$NET"

addr() { stellar keys address "tosbi-$1"; }
ADMIN=$(stellar keys address "$ADMIN_NAME")
OWNER=$(addr owner)
OPERATOR=$(addr operator)
KEEPER=$(addr keeper)
ISSUER=$(addr issuer)
TREASURY=$(addr treasury)
PRICER=$(addr pricer)
SIGNER_TOSBI=$(addr signer-tosbi)
SIGNER_PDX=$(addr signer-pdx)

stellar tx new set-options --source "$ADMIN_NAME" --signer "$SIGNER_TOSBI" --signer-weight 1 --network "$NET"
stellar tx new set-options --source "$ADMIN_NAME" --signer "$SIGNER_PDX" --signer-weight 1 --network "$NET"
stellar tx new set-options --source "$ADMIN_NAME" --master-weight 0 --low-threshold 2 --med-threshold 2 --high-threshold 2 --network "$NET"

ASSET="$CODE:$ISSUER"
for k in owner treasury; do
  stellar tx new change-trust --source "tosbi-$k" --line "$ASSET" --network "$NET"
done
stellar contract asset deploy --asset "$ASSET" --source tosbi-issuer --network "$NET"
TOKEN=$(stellar contract id asset --asset "$ASSET" --network "$NET")
(cd "$ROOT" && stellar contract build >/dev/null)

if [[ "$NET" == "local" ]]; then
  ORACLE=$(stellar contract deploy --wasm "$ROOT/target/wasm32v1-none/release/tosbi_mock_oracle.wasm" --source-account tosbi-operator --network "$NET" --alias tosbi-oracle)
  NOW=$(date +%s)
  stellar contract invoke --id "$ORACLE" --source-account tosbi-operator --network "$NET" -- set --asset XLM --timestamp "$NOW" --price "20000000000000"
  stellar contract invoke --id "$ORACLE" --source-account tosbi-operator --network "$NET" -- set --asset BTC --timestamp "$NOW" --price "10000000000000000000"
  stellar contract invoke --id "$ORACLE" --source-account tosbi-operator --network "$NET" -- set --asset ETH --timestamp "$NOW" --price "400000000000000000"
else
  ORACLE="CCYOZJCOPG34LLQQ7N24YXBM7LL62R7ONMZ3G6WZAAYPB5OYKOMJRN63"
fi

PERPDEX=$(stellar contract deploy --wasm "$ROOT/target/wasm32v1-none/release/tosbi_mock_perpdex.wasm" --source-account tosbi-operator --network "$NET" --alias tosbi-perpdex -- --oracle "$ORACLE")

if [[ "$NET" == "local" ]]; then
  MIN_POSITION_AGE=0
  ACTIVATION_DELAY=0
else
  MIN_POSITION_AGE=60
  ACTIVATION_DELAY=300
fi

TIMELOCK=300

PARAMS=$(
  cat <<EOF
{
  "a_min": "$((20 * U))",
  "a_max": "$((50000 * U))",
  "wallet_cap_bps": 200,
  "wallet_cap_max": "$((25000 * U))",
  "activation_delay": $ACTIVATION_DELAY,
  "withdraw_delay": 604800,
  "util_cap_bps": 10000,
  "side_cap_bps": 10000,
  "asset_cap_bps": 10000,
  "mark_tol_bps": 50,
  "close_tol_bps": 150,
  "min_position_age": $MIN_POSITION_AGE,
  "record_cap_bps": 1500,
  "stale_max": 7200,
  "big_payout": "$((10000 * U))",
  "share_pdx_bps": 0,
  "timelock": $TIMELOCK
}
EOF
)

PRICER_HEX=$(cd "$ROOT/pricer" && node --input-type=module -e \
  "import {StrKey} from '@stellar/stellar-sdk'; console.log(Buffer.from(StrKey.decodeEd25519PublicKey('$PRICER')).toString('hex'))")

CONFIG=$(
  cat <<EOF
{
  "admin": "$ADMIN",
  "owner": "$OWNER",
  "operator": "$OPERATOR",
  "token": "$TOKEN",
  "treasury": "$TREASURY",
  "pricer": "$PRICER_HEX",
  "oracle": "$ORACLE",
  "params": $PARAMS
}
EOF
)

VAULT=$(stellar contract deploy --wasm "$ROOT/target/wasm32v1-none/release/tosbi_protection.wasm" --source-account tosbi-owner --network "$NET" --alias tosbi-vault -- --config "$CONFIG" --assets '["XLM","BTC","ETH"]' --perpdexes "[\"$PERPDEX\"]")

stellar tx new payment --source-account tosbi-issuer --destination "$OWNER" --asset "$ASSET" --amount "$POOL" --network "$NET"
stellar contract invoke --id "$VAULT" --source-account tosbi-owner --network "$NET" -- deposit_capital --from "$OWNER" --amount "$POOL"

cat >"$ROOT/pricer/.dev.vars" <<VARS
RPC_URL=$RPC
NETWORK_PASSPHRASE=$PASS
VAULT_ID=$VAULT
PERPDEX_ID=$PERPDEX
ORACLE_ID=$ORACLE
ADMIN_ID=$ADMIN
ASSET_CODE=$CODE
ASSET_ISSUER=$ISSUER
PRICER_SECRET=$(stellar keys secret tosbi-pricer)
FAUCET_SECRET=$(stellar keys secret tosbi-issuer)
KEEPER_SECRET=$(stellar keys secret tosbi-keeper)
OPERATOR_SECRET=$(stellar keys secret tosbi-operator)
VARS

echo "network     $NET"
echo "vault       $VAULT"
echo "token       $TOKEN ($ASSET)"
echo "admin       $ADMIN"
echo "treasury    $TREASURY"
echo "pricer key  $PRICER ($PRICER_HEX)"
echo "wrote pricer/.dev.vars"

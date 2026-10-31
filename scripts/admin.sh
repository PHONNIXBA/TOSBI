#!/usr/bin/env bash
set -euo pipefail

NET=${1:?usage: admin.sh <network> <function> [args...]}
FN=${2:?usage admin.sh <network> <fuction> [args...]}
shift 2

ROOT=$(cd "$(dirname "$0")/.." && pwd)
VARS="$ROOT/pricer/.dev.vars"

VAULT=$(sed -n 's/^VAULT_ID=//p' "$VARS")
ADMIN=$(sed -n 's/^ADMIN_ID=//p' "$VARS")

if [[ -z "$VAULT" || -z "$ADMIN" ]]; then
  echo "VAULT_ID or ADMIN_ID is missing" >&2
  exit 1
fi

stellar contract invoke --id "$VAULT" --source-account "$ADMIN" --network "$NET" --build-only -- "$FN" "$@" |
  stellar tx simulate --source-account "$ADMIN" --network "$NET" |
  stellar tx sign --sign-with-key tosbi-signer-tosbi --network "$NET" |
  stellar tx sign --sign-with-key tosbi-signer-pdx --network "$NET" |
  stellar tx send --network "$NET"

#!/usr/bin/env bash
set -euo pipefail
valid_output="$(cargo run --quiet -p agentverify-cli -- contract validate examples/valid/create_customer.json --json)"
python3 -c 'import json,sys; v=json.load(sys.stdin); assert v["valid"] is True; assert v["contract_id"]' <<<"$valid_output"
invalid_output="$(mktemp)"
if cargo run --quiet -p agentverify-cli -- contract validate examples/invalid/missing_postconditions.json --json >"$invalid_output"; then
  echo "invalid example unexpectedly validated" >&2
  exit 1
fi
python3 -c 'import json,sys; v=json.load(sys.stdin); assert v["valid"] is False; assert v["errors"]' <"$invalid_output"
echo "contract examples validated: valid accepted, invalid rejected"

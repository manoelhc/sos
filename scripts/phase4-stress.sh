#!/usr/bin/env bash
set -euo pipefail

ITERATIONS="${1:-100}"

if ! [[ "$ITERATIONS" =~ ^[0-9]+$ ]] || [[ "$ITERATIONS" -eq 0 ]]; then
  echo "usage: $0 [iterations>0]"
  exit 1
fi

echo "[phase4-stress] iterations: $ITERATIONS"

for i in $(seq 1 "$ITERATIONS"); do
  echo "[phase4-stress] iteration $i/$ITERATIONS"

  cargo test -q --release test_phase4_high_load_transaction_stress
  cargo test -q --release test_phase4_epoch_is_monotonic_under_updates
  cargo test -q --release --features tls13 test_phase4_high_load_transaction_stress
  cargo test -q --release --features tls13 test_phase4_epoch_is_monotonic_under_updates
done

echo "[phase4-stress] completed successfully"

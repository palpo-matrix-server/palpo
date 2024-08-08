#!/usr/bin/env bash

# ./complement.sh  /mnt/d/Soncai/palpo-rs/complement  ./log.txt r.json
set -euo pipefail

# Path to Complement's source code
COMPLEMENT_SRC="$1"

# A `.jsonl` file to write test logs to
LOG_FILE="$2"

# A `.jsonl` file to write test results to
RESULTS_FILE="$3"

OCI_IMAGE="complement-palpo:dev"

env \
    -C "$(git rev-parse --show-toplevel)" \
    docker build \
        --tag "$OCI_IMAGE" \
        --file tests/complement/Dockerfile \
        .

# It's okay (likely, even) that `go test` exits nonzero
set +o pipefail
env \
    -C "$COMPLEMENT_SRC" \
    COMPLEMENT_BASE_IMAGE="$OCI_IMAGE" \
    go test -json ./tests | tee "$LOG_FILE"
set -o pipefail

# Post-process the results into an easy-to-compare format
cat "$LOG_FILE" | jq -c '
    select(
        (.Action == "pass" or .Action == "fail" or .Action == "skip")
        and .Test != null
    ) | {Action: .Action, Test: .Test}
    ' | sort > "$RESULTS_FILE"

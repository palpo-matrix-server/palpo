#!/usr/bin/env bash

# ./tests/complement.sh ../complement  ./__test_output_csapi.detail.txt  ./__test_output_csapi.result.jsonl
set -euo pipefail

# Path to Complement's source code
COMPLEMENT_SRC="$1"

# A `.jsonl` file to write test logs to
LOG_FILE="$2"

# A `.jsonl` file to write test results to
RESULTS_FILE="$3"

OCI_IMAGE="complement-palpo:dev"

# Complement tests that are skipped due to flakiness/reliability issues
SKIPPED_COMPLEMENT_TESTS='-skip=TestClientSpacesSummary.*|TestJoinFederatedRoomFromApplicationServiceBridgeUser.*|TestJumpToDateEndpoint.*|TestJson/Parallel/Invalid_numerical_values'

env \
    -C "$(git rev-parse --show-toplevel)" \
    docker build \
        --tag "$OCI_IMAGE" \
        --file tests/complement/Dockerfile \
        .

# It's okay (likely, even) that `go test` exits nonzero
set +o pipefail

# go test -tags="palpo_blacklist" "$SKIPPED_COMPLEMENT_TESTS" -timeout 1h -run '^(TestOutboundFederationSend)$' -json ./tests/csapi | tee "$LOG_FILE.jsonl"

env \
    -C "$COMPLEMENT_SRC" \
    COMPLEMENT_ALWAYS_PRINT_SERVER_LOGS=1 \
    COMPLEMENT_BASE_IMAGE="$OCI_IMAGE" \
    go test -tags="palpo_blacklist" "$SKIPPED_COMPLEMENT_TESTS" -timeout 1h -json ./tests/csapi | tee "$LOG_FILE.jsonl"
set -o pipefail

# Post-process the results into an easy-to-compare format
cat "$LOG_FILE.jsonl" | jq -c '
    select(
        (.Action == "pass" or .Action == "fail" or .Action == "skip")
        and .Test != null
    ) | {Action: .Action, Test: .Test}
    ' | sort > "$RESULTS_FILE"

cat "$LOG_FILE.jsonl" | jq -c '.Output' > "$LOG_FILE"
rm -rf "$LOG_FILE.jsonl"
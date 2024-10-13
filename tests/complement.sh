#!/usr/bin/env bash

# ./tests/complement.sh ../complement  ./__test_tmp.detail.txt  ./__test_tmp.result.jsonl
set -euo pipefail

# Path to Complement's source code
COMPLEMENT_SRC="$1"

# A `.jsonl` file to write test logs to
LOG_FILE="$2"

# A `.jsonl` file to write test results to
RESULTS_FILE="$3"

BASE_IMAGE="complement-palpo-base"
if [ -z "$(docker images -q $BASE_IMAGE)" ]; then
    echo "Image $BASE_IMAGE is not exist, build it..."
    env \
    -C "$(git rev-parse --show-toplevel)" \
    docker build -t complement-palpo-base -f tests/complement/Dockerfile.base .
else
    echo "Image $BASE_IMAGE is exists, skip building..."
fi

TEST_IMAGE="complement-palpo-test"

# Complement tests that are skipped due to flakiness/reliability issues
SKIPPED_COMPLEMENT_TESTS='-skip=TestClientSpacesSummary.*|TestJoinFederatedRoomFromApplicationServiceBridgeUser.*|TestJumpToDateEndpoint.*|TestJson/Parallel/Invalid_numerical_values'

env \
    -C "$(git rev-parse --show-toplevel)" \
    DOCKER_BUILDKIT=1 docker build --tag "$TEST_IMAGE" --file tests/complement/Dockerfile.test .

# It's okay (likely, even) that `go test` exits nonzero
set +o pipefail

# go test -tags="palpo_blacklist" "$SKIPPED_COMPLEMENT_TESTS" -timeout 1h -run '^(TestOutboundFederationSend)$' -json ./tests/csapi | tee "$LOG_FILE.jsonl"

env \
    -C "$COMPLEMENT_SRC" \
    COMPLEMENT_ALWAYS_PRINT_SERVER_LOGS=1 \
    COMPLEMENT_BASE_IMAGE="$TEST_IMAGE" \
    go test -tags="palpo_blacklist" "$SKIPPED_COMPLEMENT_TESTS" -timeout 1h -run "TestDeviceListUpdates/when_leaving_a_room_with_a_local_user" -json ./tests/csapi| tee "$LOG_FILE.jsonl"
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
#!/usr/bin/env bash
#
# Runs SyTest either from Docker Hub, or from ../sytest. If it's run
# locally, the Docker image is rebuilt first.
#
# Logs are stored in ../sytestout/logs.

set -e
set -o pipefail

main() {
    local tag=buster
    local base_image=debian:$tag
    local runargs=()
    local src=$PWD

    cd "$(dirname "$0")"

    if [ -d ../sytest ]; then
        local tmpdir
        tmpdir="$(mktemp -d -t run-systest.XXXXXXXXXX)"
        echo "Using temporary directory $tmpdir"
        trap "rm -r '$tmpdir'" EXIT

        if [ -z "$DISABLE_BUILDING_SYTEST" ]; then
            echo "Re-building ../sytest Docker images..."

            # local status
            # (
                cd ../sytest

                # docker build -f docker/base.Dockerfile --build-arg BASE_IMAGE="$base_image" --tag matrixdotorg/sytest:"$tag" .

                cd ../sytest-palpo
                docker build -f palpo.Dockerfile --build-arg SYTEST_IMAGE_TAG="$tag" --tag matrixdotorg/sytest-palpo:latest .
            # ) &>"$tmpdir/buildlog" || status=$?
            # if (( status != 0 )); then
            #     # Docker is very verbose, and we don't really care about
            #     # building SyTest. So we accumulate and only output on
            #     # failure.
            #     cat "$tmpdir/buildlog" >&2
            #     return $status
            # fi
        fi

        runargs+=( -v "$src/../sytest:/sytest" )
    fi
    # if [ -n "$SYTEST_POSTGRES" ]; then
        runargs+=( -e POSTGRES=1 )
    # fi

    local sytestout=$src/sytestout
    local sytestplugin=$src/../sytest-palpo
    mkdir -p "$sytestout/logs"
    docker run \
           --rm \
           --name "sytest-palpo-${LOGNAME}" \
           -e LOGS_USER=$(id -u) \
           -e LOGS_GROUP=$(id -g) \
           -v "$src:/src/" \
           -v "$sytestout/logs:/logs/" \
           -v "$sytestplugin:/sytest/plugins/palpo/" \
           "${runargs[@]}" \
           matrixdotorg/sytest-palpo:latest "$@"
}

main "$@"

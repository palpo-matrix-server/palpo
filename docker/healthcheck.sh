#!/bin/sh

# If PALPO_LISTEN_ADDR is not set try to get the address from the process list
if [ -z "${PALPO_LISTEN_ADDR}" ]; then
  PALPO_LISTEN_ADDR=$(ss -tlpn | awk -F ' +|:' '/palpo/ { print $4 }')
fi

# The actual health check.
# We try to first get a response on HTTP and when that fails on HTTPS and when that fails, we exit with code 1.
# TODO: Change this to a single wget call. Do we have a config value that we can check for that?
wget --no-verbose --tries=1 --spider "https://${PALPO_LISTEN_ADDR}/_matrix/client/versions" || exit 1

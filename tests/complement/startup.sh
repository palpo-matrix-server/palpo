#!/bin/bash
#
# Default ENTRYPOINT for the docker image used for testing palpo under complement

# set -e

printenv

/etc/init.d/postgresql start
uname -a
sed -i "s/server_name = \"your.server.name\"/server_name = \"${SERVER_NAME}\"/g" /work/palpo.toml
sed -i "s/your.server.name/${SERVER_NAME}/g" /work/caddy.json
/work/palpo


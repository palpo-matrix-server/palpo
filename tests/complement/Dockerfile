FROM rust:latest

WORKDIR /palpo

RUN apt-get update && apt-get install -y --no-install-recommends \
    libclang-dev postgresql postgresql-contrib



COPY Cargo.toml Cargo.toml
COPY Cargo.lock Cargo.lock
COPY crates crates
RUN cargo build --release \
    && mv target/release/palpo palpo \
    && rm -rf target

# Install caddy
RUN apt-get update \
    && apt-get install -y \
    debian-keyring \
    debian-archive-keyring \
    apt-transport-https \
    curl \
    && curl -1sLf 'https://dl.cloudsmith.io/public/caddy/testing/gpg.key' \
    | gpg --dearmor -o /usr/share/keyrings/caddy-testing-archive-keyring.gpg \
    && curl -1sLf 'https://dl.cloudsmith.io/public/caddy/testing/debian.deb.txt' \
    | tee /etc/apt/sources.list.d/caddy-testing.list \
    && apt-get update \
    && apt-get install -y caddy

COPY tests/palpo.toml palpo.toml
COPY tests/complement/caddy.json caddy.json

ENV PALPO_DATABASE_URL="postgres://postgres:postgres@127.0.0.1:5432/palpo"
ENV PALPO_SERVER_NAME=localhost
ENV PALPO_CONFIG=/palpo/palpo.toml

RUN sed -i "s/port = 6167/port = 8008/g" palpo.toml
RUN echo "log = \"warn,_=off,sled=off\"" >> palpo.toml
RUN sed -i "s/address = \"127.0.0.1\"/address = \"0.0.0.0\"/g" palpo.toml
RUN sed -i "s%127.0.0.1/32            md5%127.0.0.1/32            trust%g" /etc/postgresql/15/main/pg_hba.conf && \
    # Bump up max conns for moar concurrency
    sed -i 's/max_connections = 100/max_connections = 2000/g' /etc/postgresql/15/main/postgresql.conf


# This entry script starts postgres, waits for it to be up then starts palpo
RUN echo '\
    #!/bin/bash -eu \n\
    pg_lsclusters \n\
    pg_ctlcluster 13 main start \n\
    \n\
    until pg_isready \n\
    do \n\
    echo "Waiting for postgres"; \n\
    sleep 1; \n\
    done \n\
    ' > run_postgres.sh && chmod +x run_postgres.sh

RUN /etc/init.d/postgresql start && \
    su - postgres -c "psql -c \"ALTER USER postgres WITH PASSWORD 'postgres';\""

RUN /etc/init.d/postgresql start && \
    su - postgres -c "createdb palpo"

EXPOSE 8008 8448

CMD uname -a && \
    sed -i "s/#server_name = \"your.server.name\"/server_name = \"${SERVER_NAME}\"/g" palpo.toml && \
    sed -i "s/your.server.name/${SERVER_NAME}/g" caddy.json && \
    /palpo/palpo

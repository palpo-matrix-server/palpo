# syntax=docker/dockerfile:1
# ---------------------------------------------------------------------------------------------------------
# This Dockerfile is intended to be built as part of Palpo's CI pipeline.
# It does not build Palpo in Docker, but just copies the matching build artifact from the build jobs.
#
# It is mostly based on the normal Palpo Dockerfile, but adjusted in a few places to maximise caching.
# Credit's for the original Dockerfile: Weasy666.
# ---------------------------------------------------------------------------------------------------------

FROM docker.io/alpine:3.16.0@sha256:4ff3ca91275773af45cb4b0834e12b7eb47d1c18f770a0b151381cd227f4c253 AS runner


# Standard port on which Palpo launches.
# You still need to map the port when using the docker command or docker-compose.
EXPOSE 6167

# Users are expected to mount a volume to this directory:
ARG DEFAULT_DB_PATH=/var/lib/matrix-palpo

ENV PALPO_LISTEN_ADDR="0.0.0.0:6167" \
    PALPO_DATABASE_PATH=${DEFAULT_DB_PATH} \
    PALPO_CONFIG=''
#    └─> Set no config file to do all configuration with env vars

# Palpo needs:
#   ca-certificates: for https
#   iproute2: for `ss` for the healthcheck script
RUN apk add --no-cache \
    ca-certificates \
    iproute2

ARG CREATED
ARG VERSION
ARG GIT_REF
# Labels according to https://github.com/opencontainers/image-spec/blob/master/annotations.md
# including a custom label specifying the build command
LABEL org.opencontainers.image.created=${CREATED} \
    org.opencontainers.image.authors="Palpo Contributors" \
    org.opencontainers.image.title="Palpo" \
    org.opencontainers.image.version=${VERSION} \
    org.opencontainers.image.vendor="Palpo Contributors" \
    org.opencontainers.image.description="A Matrix homeserver written in Rust" \
    org.opencontainers.image.url="https://palpo.chat/" \
    org.opencontainers.image.revision=${GIT_REF} \
    org.opencontainers.image.source="https://gitlab.com/famedly/palpo.git" \
    org.opencontainers.image.licenses="Apache-2.0" \
    org.opencontainers.image.documentation="https://gitlab.com/famedly/palpo" \
    org.opencontainers.image.ref.name=""


# Test if Palpo is still alive, uses the same endpoint as Element
COPY ./docker/healthcheck.sh /srv/palpo/healthcheck.sh
HEALTHCHECK --start-period=5s --interval=5s CMD ./healthcheck.sh

# Improve security: Don't run stuff as root, that does not need to run as root:
# Most distros also use 1000:1000 for the first real user, so this should resolve volume mounting problems.
ARG USER_ID=1000
ARG GROUP_ID=1000
RUN set -x ; \
    deluser --remove-home www-data ; \
    addgroup -S -g ${GROUP_ID} palpo 2>/dev/null ; \
    adduser -S -u ${USER_ID} -D -H -h /srv/palpo -G palpo -g palpo palpo 2>/dev/null ; \
    addgroup palpo palpo 2>/dev/null && exit 0 ; exit 1

# Change ownership of Palpo files to palpo user and group
RUN chown -cR palpo:palpo /srv/palpo && \
    chmod +x /srv/palpo/healthcheck.sh && \
    mkdir -p ${DEFAULT_DB_PATH} && \
    chown -cR palpo:palpo ${DEFAULT_DB_PATH}

# Change user to palpo
USER palpo
# Set container home directory
WORKDIR /srv/palpo

# Run Palpo and print backtraces on panics
ENV RUST_BACKTRACE=1
ENTRYPOINT [ "/srv/palpo/palpo" ]

# Depending on the target platform (e.g. "linux/arm/v7", "linux/arm64/v8", or "linux/amd64")
# copy the matching binary into this docker image
ARG TARGETPLATFORM
COPY --chown=palpo:palpo ./$TARGETPLATFORM /srv/palpo/palpo

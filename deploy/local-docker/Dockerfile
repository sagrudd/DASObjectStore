# syntax=docker/dockerfile:1.7

FROM rust:1.88-bookworm AS builder

RUN apt-get update \
    && apt-get install --no-install-recommends -y \
        build-essential \
        clang \
        libclang-dev \
        libpam0g-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY DASObjectStore /src/DASObjectStore
COPY prosopikon /src/prosopikon
WORKDIR /src/DASObjectStore
RUN sed -i \
        's#prosopikon-core = { git = "https://github.com/sagrudd/prosopikon.git", rev = "c1dd477270e3ab34f5444e2c0565af845a930a70" }#prosopikon-core = { path = "../prosopikon/crates/prosopikon-core" }#' \
        Cargo.toml \
    && cargo build --release -p dasobjectstore-cli -p dasobjectstore-daemon

FROM docker:27.5.1-cli AS docker-cli

FROM almalinux:9-minimal

ARG DASOBJECTSTORE_SOURCE_COMMIT=unavailable
LABEL org.opencontainers.image.source="https://github.com/sagrudd/DASObjectStore"
LABEL org.opencontainers.image.revision="$DASOBJECTSTORE_SOURCE_COMMIT"

WORKDIR /opt/dasobjectstore
RUN microdnf install -y \
        ca-certificates \
        libstdc++ \
        openssl-libs \
        pam \
        zlib \
    && microdnf clean all
COPY --from=builder /src/DASObjectStore/target/release/dasobjectstore /usr/local/bin/dasobjectstore
COPY --from=builder /src/DASObjectStore/target/release/dasobjectstored /usr/local/bin/dasobjectstored
COPY --from=docker-cli /usr/local/bin/docker /usr/local/bin/docker
COPY --from=docker-cli /usr/local/libexec/docker/cli-plugins/docker-compose /usr/local/libexec/docker/cli-plugins/docker-compose

RUN docker compose version

ENTRYPOINT ["dasobjectstored"]

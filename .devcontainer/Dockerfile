# Entorno de desarrollo y pruebas de pkg-merge.
# No forma parte del binario final: solo se usa para compilar y ejecutar tests.
FROM rust:1-slim-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        gcc \
        libc6-dev \
        libssl-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt clippy

WORKDIR /work

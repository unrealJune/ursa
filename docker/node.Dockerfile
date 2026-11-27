# syntax=docker/dockerfile:1
# ursa-node: build with the workspace, run on distroless as a non-root user.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY .cargo .cargo
COPY crates crates
# The default relay token is baked in at build time (crates/ursa-proto/src/relays.rs). It comes in as
# a BuildKit secret, so it is in no layer or build arg; RELAY_TOKEN_ID (a short hash of it) only
# makes a new token rebuild instead of reusing a cached binary. Without the secret: no token.
ARG RELAY_TOKEN_ID=none
RUN --mount=type=secret,id=relay_token,env=URSA_DEFAULT_RELAY_TOKEN \
    echo "relay token: ${RELAY_TOKEN_ID}" && cargo build --release --locked -p ursa-node -p ursa-cli \
 && cp target/release/ursa-node target/release/ursa /

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /ursa-node /usr/local/bin/ursa-node
# The CLI rides along for in-cluster tests and administration (`kubectl exec ... ursa status`).
COPY --from=build /ursa /usr/local/bin/ursa
COPY LICENSE /usr/share/doc/ursa/LICENSE
USER nonroot:nonroot
# 8080: plain-HTTP health for probes. The iroh UDP port is set with URSA_PORT.
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/ursa-node"]

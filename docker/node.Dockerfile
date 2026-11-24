# ursa-node: build with the workspace, run on distroless as a non-root user.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY .cargo .cargo
COPY crates crates
RUN cargo build --release --locked -p ursa-node \
 && cp target/release/ursa-node /ursa-node

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /ursa-node /usr/local/bin/ursa-node
USER nonroot:nonroot
# 8080: plain-HTTP health for probes. The iroh UDP port is set with URSA_PORT.
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/ursa-node"]

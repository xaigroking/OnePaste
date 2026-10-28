# The OnePaste server as a container image.
#
# The final image is distroless (the C library and nothing else: no shell, no package manager), runs as the unprivileged
# user 65532, and keeps all its data in the volume /data. Build: docker build -t onepaste-server .
FROM rust:1-bookworm AS build
ARG BUILD_TAG=""
ENV ONEPASTE_BUILD_TAG=${BUILD_TAG}
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY tools ./tools
# Only the server: no test-only features
RUN cargo build --release --locked -p onepaste-server
# The data directory is made here, owned by the unprivileged user, because the final image has no shell
RUN mkdir /data

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/onepaste-server /onepaste-server
COPY --from=build --chown=65532:65532 /data /data
ENV ONEPASTE_DATA_DIR=/data
USER 65532:65532
VOLUME /data
EXPOSE 8443
# The server has no health endpoint, so the check is a TCP connection to its own listen port
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 CMD ["/onepaste-server", "healthcheck"]
ENTRYPOINT ["/onepaste-server"]

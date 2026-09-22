FROM rust:1.85-bookworm AS builder
WORKDIR /src

COPY Cargo.toml build.rs ./
COPY proto ./proto
RUN mkdir -p src && printf 'fn main() {}\n' > src/main.rs
RUN cargo fetch

COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home --home-dir /var/lib/spm-cloud spm-cloud
WORKDIR /var/lib/spm-cloud
COPY --from=builder /src/target/release/spm-cloud /usr/local/bin/spm-cloud
RUN mkdir -p /var/lib/spm-cloud/data /var/lib/spm-cloud/objects \
    && chown -R spm-cloud:spm-cloud /var/lib/spm-cloud
USER spm-cloud
ENV SPM_CLOUD_BIND=0.0.0.0:8787 \
    SPM_CLOUD_DATA_DIR=/var/lib/spm-cloud/data \
    SPM_CLOUD_OBJECT_DIR=/var/lib/spm-cloud/objects
EXPOSE 8787
ENTRYPOINT ["/usr/local/bin/spm-cloud"]

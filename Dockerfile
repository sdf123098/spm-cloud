FROM rust:1.87-bookworm AS builder
WORKDIR /src

COPY Cargo.toml Cargo.lock build.rs ./
COPY proto ./proto
RUN mkdir -p src && printf 'fn main() {}\n' > src/main.rs
RUN cargo fetch --locked

COPY src ./src
COPY web ./web
COPY config.schema.json ./config.schema.json
COPY assets/official-profile-certificate-keys.json ./assets/official-profile-certificate-keys.json
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home --home-dir /var/lib/spm-cloud spm-cloud
WORKDIR /var/lib/spm-cloud
COPY --from=builder /src/target/release/spm-cloud /usr/local/bin/spm-cloud
COPY config.example.json config.schema.json nginx-cloud.conf.example /usr/share/doc/spm-cloud/
COPY docs/PROTOCOL_V1.md /usr/share/doc/spm-cloud/PROTOCOL_V1.md
COPY web/credits.txt /usr/share/doc/spm-cloud/THIRD_PARTY_NOTICES.txt
RUN mkdir -p /var/lib/spm-cloud/data /var/lib/spm-cloud/objects \
    && chown -R spm-cloud:spm-cloud /var/lib/spm-cloud
USER spm-cloud
ENV SPM_CLOUD_CONTAINER=1 \
    SPM_CLOUD_BIND=0.0.0.0:8787 \
    SPM_CLOUD_DATA_DIR=/var/lib/spm-cloud/data \
    SPM_CLOUD_OBJECT_DIR=/var/lib/spm-cloud/objects
EXPOSE 8787
ENTRYPOINT ["/usr/local/bin/spm-cloud"]

# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY webui ./webui
RUN cargo build --release --bin guche

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /src/target/release/guche /usr/local/bin/guche
COPY config.default.json /app/config.default.json
# config.json 与 data/ 请挂载到 /app 下
EXPOSE 8901 8080
CMD ["guche"]

FROM rust:1.82 AS builder

WORKDIR /build
COPY Cargo.lock .
COPY Cargo.toml .
RUN mkdir src
RUN echo "fn main() {}" > src/main.rs
RUN cargo fetch
COPY src src
RUN cargo build --release


FROM debian:latest
RUN apt-get update
RUN apt-get install curl --yes
COPY --from=builder build/target/release/rollouts /usr/bin/
ENTRYPOINT ["rollouts"]

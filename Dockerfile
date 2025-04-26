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
RUN apt update
RUN apt install docker.io --yes
RUN apt install docker-compose --yes
RUN apt install curl --yes
COPY --from=builder build/target/release/rollouts /usr/bin/
ENTRYPOINT ["rollouts"]

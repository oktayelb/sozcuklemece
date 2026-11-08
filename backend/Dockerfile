FROM rust:1.96-bookworm AS build

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked --bin sozcuklemece-backend \
    && cp target/release/sozcuklemece-backend /sozcuklemece-backend
RUN mkdir /data

FROM gcr.io/distroless/cc-debian12:nonroot

COPY --from=build /sozcuklemece-backend /usr/local/bin/sozcuklemece-backend
COPY --from=build --chown=65532:65532 /data /app/data

WORKDIR /app
ENV BIND_ADDRESS=0.0.0.0:8000 \
    WORD_FILE=/app/data/current_word.json
VOLUME /app/data
EXPOSE 8000

ENTRYPOINT ["/usr/local/bin/sozcuklemece-backend"]

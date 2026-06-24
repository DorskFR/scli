# Build a static musl binary, ship it on scratch.
# TLS roots are bundled via rustls + webpki-roots, so no CA certs needed at runtime.
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY . .
RUN cargo build --release

FROM scratch
COPY --from=build /app/target/release/scli /scli
ENTRYPOINT ["/scli"]

FROM rust as build

WORKDIR /build
COPY . .
RUN cargo build --release

FROM debian:bullseye-slim

COPY --from=build /build/target/release/passwordle .
CMD ["./passwordle"]

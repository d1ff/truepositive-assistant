FROM rust:1.44-alpine3.12 as builder
WORKDIR /usr/src/truepositive-assistant
COPY . .
RUN apk add --no-cache musl-dev
RUN cargo install --path .

FROM alpine:3.12
RUN apk add --no-cache ca-certificates
COPY --from=builder /usr/local/cargo/bin/truepositive-assistant /usr/local/bin/truepositive-assistant
EXPOSE 5000
CMD ["truepositive-assistant"]

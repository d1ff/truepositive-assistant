FROM rust:1.44-buster as builder
WORKDIR /usr/src/truepositive-assistant
COPY . .
RUN cargo install --path .

FROM debian:buster-slim
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get -y install ca-certificates libssl-dev && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/truepositive-assistant /usr/local/bin/truepositive-assistant
EXPOSE 5000
CMD ["truepositive-assistant"]

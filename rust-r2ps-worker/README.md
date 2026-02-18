# Rust R2PS worker

## dev tools

Install rust toolchain from [rustup](https://rustup.rs/).

Install the following rust-rdkafka
dependencies ( [rdkafka installation instructions](https://github.com/fede1024/rust-rdkafka?tab=readme-ov-file#installation) )
to build the project locally:

### Debian/Ubuntu

```bash
apt-get update && apt-get install -y \
    zlib1g zlib1g-dev \
    cmake \
    libssl-dev \
    libsasl2-dev \
    libzstd-dev
```

### OSX

```bash
brew install \
    zlib \
    cmake \
    openssl \
    cyrus-sasl \
    zstd \
    softhsm
```

## configuration

### softhsm tokens

Start the ecosystem (in docker compose) including the rust-r2ps-worker by running:
An AES unwrap key is created in softhsm on first start of the container in a persistent volume.
The same key is required in local environment in order to use same HsmState as in docker compose.

```bash
make up
make copy-tokens
```

Make sure the [softhsm2.conf](./softhsm/softhsm2.conf) specifies the location of the copied token dir

```text
directories.tokendir = ../softhsm-tokens/
```

### opaque and server configuration

Create a `.env` file with environment variables. Make sure the same values are used for opaque setup as in compose (see
`.env.softhsm` and `.env.opaque`)

Basic initial setup should be found in `.env-mac` and `.env_linux`

## build and run

Then build and run the rust-r2ps-worker with:

```bash
cargo run
```

## Docs and openapi

Generate [./docs/domain-model.html](./docs/book/book/introduction.html) and [./openapi.json](./openapi.json)

```
make docs
```

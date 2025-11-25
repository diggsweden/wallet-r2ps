# Rust implementation of R2PS worker

work in progress...

## Build and run the project locally

Install rust toolchain from [rustup](https://rustup.rs/).

Start the ecosystem with docker compose and stop the rust-r2ps-worker by running:
```bash
docker compose up 
docker compose down rust-r2ps-worker
```

Install the following rust-rdkafka dependencies ( [rdkafka installation instructions](https://github.com/fede1024/rust-rdkafka?tab=readme-ov-file#installation) ) in order to build the project locally:

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
    zstd
```

Then build and run the rust-r2ps-worker with:
```bash
cargo run
```

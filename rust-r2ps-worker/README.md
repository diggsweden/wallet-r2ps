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

### HSM key setup (first time)

The worker uses HSM-backed key derivation. Two persistent keys must exist in the HSM slot before starting the service: an AES wrapping key (protects generated device keys at rest) and an HMAC root key (seeds all derived JWS and OPAQUE keys).

All `hsm-*` targets run the keytool locally by default. Add `DOCKER=1` to run inside the Docker container instead:

```bash
make hsm-setup DOCKER=1
```

First-time setup (creates both keys and prints the derived public keys):

```bash
SOFTHSM2_CONF=softhsm/softhsm2.conf make hsm-setup
```

Or individually:

```bash
# Create the AES-256 wrapping key (once per slot)
SOFTHSM2_CONF=softhsm/softhsm2.conf make hsm-create-wrapping-key

# Create the HMAC root key (pin the label with HSM_ROOT_KEY_LABEL=rk-YYYYMM format (suggestion))
SOFTHSM2_CONF=softhsm/softhsm2.conf make hsm-create-root-key HSM_ROOT_KEY_LABEL=rk-202601

# Print the derived JWS and OPAQUE public keys
SOFTHSM2_CONF=softhsm/softhsm2.conf make hsm-derive-public-keys HSM_ROOT_KEY_LABEL=rk-202601
```

Check that both keys are present and functional:

```bash
SOFTHSM2_CONF=softhsm/softhsm2.conf make hsm-status HSM_ROOT_KEY_LABEL=rk-202601 VERBOSE=1
```

Enable key derivation in `.env` by setting (uncomment and fill in with the label used above):

```
HSM_ROOT_KEY_LABEL=rk-202601
JWS_DOMAIN_SEPARATOR=rk-202601_jws-v1
OPAQUE_DOMAIN_SEPARATOR=rk-202601_opaque-v1
```

When these variables are set the legacy `SERVER_PRIVATE_KEY` value is ignored.

`OPAQUE_SERVER_SETUP` is still required in both legacy and HSM key derivation modes. The OPAQUE
`ServerSetup` contains an OPRF key that is randomly generated on first startup and is independent
of the server signing keypair. If the OPRF key changes, all existing client registrations are
permanently invalidated. On first startup the service logs `OPAQUE_SERVER_SETUP=<base64>` — that
value must be saved and set in the environment for all subsequent starts.

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

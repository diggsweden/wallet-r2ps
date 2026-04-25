# SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
#
# SPDX-License-Identifier: EUPL-1.2

# Container image for running `just verify` locally without installing
# all tooling on the host. Build once; mount source at runtime.
#
# Build (run from workspace root):
#   docker build -t hsm-verify -f ci/Dockerfile .
#
# Run (from workspace root):
#   docker run --rm -v "$(pwd):/src" hsm-verify

FROM rust:1.94-bookworm@sha256:6ae102bdbf528294bc79ad6e1fae682f6f7c2a6e6621506ba959f9685b308a55

RUN apt-get update && apt-get install -y \
    git \
    cmake \
    pkg-config \
    libssl-dev \
    libsasl2-dev \
    libzstd-dev \
    zlib1g-dev \
    python3-pip \
    pipx \
    curl \
    xz-utils \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m -u 1000 verify \
    && mkdir /verify && chown verify:verify /verify

ENV HOME=/home/verify
ENV PATH="/home/verify/.local/bin:/home/verify/.local/share/mise/shims:${PATH}"
ENV MISE_YES=1
ENV MISE_EXPERIMENTAL=1

# Install mise into verify's home, then fix ownership
SHELL ["/bin/bash", "-o", "pipefail", "-c"]
RUN curl https://mise.run | sh && chown -R verify:verify /home/verify

USER verify

# Copy mise config and install all pinned tools (versions from .mise.toml)
# The rust image makes CARGO_HOME/RUSTUP_HOME world-writable so cargo builds work
WORKDIR /build
COPY --chown=verify:verify .mise.toml ./
RUN mise trust && mise install

# Clone devbase-check into the location just verify expects
RUN git clone --depth 1 https://github.com/diggsweden/devbase-check \
    /home/verify/.local/share/devbase-check

# Source is mounted here at runtime; entrypoint clones a clean copy for checks
WORKDIR /src
COPY --chown=verify:verify ci/run.sh /usr/local/bin/run.sh
RUN chmod +x /usr/local/bin/run.sh

CMD ["run.sh"]

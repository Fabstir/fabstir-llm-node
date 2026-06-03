FROM nvidia/cuda:12.4.1-devel-ubuntu22.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential pkg-config libssl-dev ca-certificates \
        curl git sudo jq ripgrep \
        clang libclang-dev llvm-dev cmake \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

ARG USERNAME=developer
ARG USER_UID=1000
ARG USER_GID=1000
RUN groupadd --gid $USER_GID $USERNAME \
    && useradd  --uid $USER_UID --gid $USER_GID -m -s /bin/bash $USERNAME \
    && echo "$USERNAME ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/$USERNAME

ENV CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:/home/developer/.risc0/bin:/home/developer/.npm-global/bin:$PATH

RUN mkdir -p $CARGO_HOME $RUSTUP_HOME \
    && chown -R $USERNAME:$USERNAME $CARGO_HOME $RUSTUP_HOME

USER $USERNAME
WORKDIR /home/$USERNAME

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --default-toolchain stable --profile minimal \
    && rustup component add rustfmt clippy

# Risc0 zkVM toolchain — required for `cargo build --features real-ezkl` (real STARK proofs).
# rzup installs to ~/.risc0/{bin,toolchains}; ~/.risc0/bin is on PATH via the ENV above.
RUN curl -fsSL https://risczero.com/install -o /tmp/rzup.sh \
    && bash /tmp/rzup.sh \
    && rm /tmp/rzup.sh \
    && ~/.risc0/bin/rzup install rust \
    && ~/.risc0/bin/rzup install cargo-risczero

RUN npm config set prefix "$HOME/.npm-global" \
    && npm config set os linux \
    && npm install -g @anthropic-ai/claude-code --force --no-os-check

RUN echo 'export PATH="$HOME/.npm-global/bin:$HOME/.risc0/bin:/usr/local/cargo/bin:$PATH"' >> ~/.bashrc

USER root
RUN mkdir -p /home/developer/.claude && chown developer:developer /home/developer/.claude
USER developer

WORKDIR /workspace
CMD ["sleep", "infinity"]
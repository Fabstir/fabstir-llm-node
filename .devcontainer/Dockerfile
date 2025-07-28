# fabstir-llm-node/.devcontainer/Dockerfile
FROM nvidia/cuda:12.3.1-devel-ubuntu22.04

# Install basic dependencies
RUN apt-get update && apt-get install -y \
    curl \
    build-essential \
    pkg-config \
    libssl-dev \
    git \
    && rm -rf /var/lib/apt/lists/*

# Install Rust system-wide
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path && \
    chmod -R a+w $RUSTUP_HOME $CARGO_HOME

# Verify installations
RUN nvcc --version && rustc --version

# Install system dependencies
RUN apt-get update && apt-get install -y \
    build-essential pkg-config libssl-dev protobuf-compiler \
    cmake clang llvm libclang-dev \
    git curl jq vim tmux inotify-tools netcat-openbsd \
    python3 python3-pip \
    && rm -rf /var/lib/apt/lists/*

# Install Node.js for tooling
RUN curl -fsSL https://deb.nodesource.com/setup_18.x | bash - \
    && apt-get install -y nodejs

# Install Rust tools (with --locked flag for stability)
RUN cargo install --locked cargo-watch && \
    cargo install --locked cargo-edit && \
    cargo install --locked cargo-expand && \
    cargo install --locked wasm-bindgen-cli && \
    cargo install --locked wasm-pack

# Install sccache
RUN cargo install sccache
ENV RUSTC_WRAPPER=sccache

WORKDIR /workspace

# Copy YOLO scripts
COPY yolo-runner.sh test-watcher.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/*.sh

# Create developer user
RUN useradd -m -s /bin/bash developer && \
    echo "developer ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers

# Switch to developer user
USER developer
WORKDIR /workspace

ENTRYPOINT ["/usr/local/bin/yolo-runner.sh"]
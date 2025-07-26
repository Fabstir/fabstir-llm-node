# fabstir-llm-node/.devcontainer/Dockerfile
FROM rust:1.88.0-bookworm

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

# Install sccache BEFORE creating developer user
RUN cargo install sccache
ENV RUSTC_WRAPPER=sccache

WORKDIR /workspace

# Copy YOLO scripts
COPY yolo-runner.sh test-watcher.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/*.sh

# Create developer user
RUN useradd -m -s /bin/bash developer && \
    echo "developer ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers

# Switch to developer user AFTER all installations
USER developer
WORKDIR /workspace

ENTRYPOINT ["/usr/local/bin/yolo-runner.sh"]
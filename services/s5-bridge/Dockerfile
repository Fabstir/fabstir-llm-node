# Enhanced S5.js Bridge Service Docker Image
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1

FROM node:20-alpine

# Install basic utilities
RUN apk add --no-cache curl

# Create app directory
WORKDIR /app

# Copy package files + the vendored s5.js tarball (self-contained build), then install
COPY package.json ./
COPY julesl23-s5js-0.9.0-beta.45.tgz ./
RUN npm install --production

# Copy application code
COPY src/ ./src/

# Expose HTTP port
EXPOSE 5522

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
  CMD curl -f http://localhost:5522/health || exit 1

# Run as non-root user
RUN addgroup -g 1001 -S s5bridge && \
    adduser -S s5bridge -u 1001 -G s5bridge && \
    chown -R s5bridge:s5bridge /app

USER s5bridge

# Start the service
CMD ["node", "src/server.js"]

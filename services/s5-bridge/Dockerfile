# Enhanced S5.js Bridge Service Docker Image
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1

FROM node:20-alpine

# Install basic utilities
RUN apk add --no-cache curl

# Create app directory
WORKDIR /app

# @julesl23/s5js resolves from the npm registry at an exact pin (0.9.0-beta.55),
# not from a vendored tarball. beta.55 is the first published version carrying the
# unbounded-fetch fix; beta.50 and beta.54 exist only as tarballs and are
# unreachable by npm, which is why the vendored-tgz build was dropped.
# npm ci (not install) so the lockfile decides every version, not just this one.
COPY package.json package-lock.json ./
RUN npm ci --omit=dev

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

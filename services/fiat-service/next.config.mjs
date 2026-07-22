/** @type {import('next').NextConfig} */
const nextConfig = {
  // API-only service: no UI, no wallet stack, no s5js. The only backend concerns
  // are the /v1/fiat/* route handlers and the FC1.3 settlement listener started
  // from instrumentation.ts. So this config is deliberately smaller than the
  // website's: no @base-org/account / sdk-core server aliases, no browser undici
  // stub. The one thing it keeps is the edge node: stub below.
  reactStrictMode: true,
  // Lets instrumentation.ts run at server start (the fiat settlement listener — a
  // request handler cannot watch chain events). Inert unless FIAT_SETTLEMENT_ENABLED=1.
  experimental: { instrumentationHook: true },
  webpack: (config, { nextRuntime, webpack }) => {
    // instrumentation.ts (the FC1.3 settlement listener) is compiled for the EDGE
    // runtime as well as nodejs. Its chain (settlement-listener → fiat-session-
    // service → ledger/fiat-credentials) imports node: builtins (node:crypto,
    // node:fs/promises, node:path) that the edge runtime can't resolve →
    // UnhandledSchemeError, which fails the whole compile (dev 500s, build fails).
    // register() returns early on any non-nodejs runtime, so on EDGE that chain is
    // dead code — rewrite node:x → x and stub those builtins so the edge compile
    // succeeds. The nodejs runtime is untouched and still gets the real modules.
    if (nextRuntime === 'edge') {
      config.plugins.push(
        new webpack.NormalModuleReplacementPlugin(/^node:/, (resource) => {
          resource.request = resource.request.replace(/^node:/, '');
        })
      );
      config.resolve.fallback = {
        ...config.resolve.fallback,
        crypto: false,
        fs: false,
        'fs/promises': false,
        path: false,
      };
    }
    return config;
  },
};

export default nextConfig;

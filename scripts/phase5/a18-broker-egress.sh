#!/usr/bin/env bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Phase 5 gate A-18: can the key broker's home reach every service on the
# release path? Run ON THE BOX THE BROKER WILL LIVE ON. Prints one line per
# endpoint with the HTTP code and PASS/FAIL, exits non-zero if any fail.
#
# Every probe is keyless. Expected codes verified from the dev box 2026-09-17.
#   PCCS (dcap-qvl default, Phala)      GET  rootcacrl              -> 200
#   Intel PCS (fallback collateral)     GET  tdx qe/identity        -> 200
#   Intel root CA (collateral chain)    GET  IntelSGXRootCA.der     -> 200
#   NRAS GPU verifier (v3, SDK default) POST {} (empty body)        -> 400  (route exists; a real payload gets 200)
#   NRAS JWKS (EAT signature keys)      GET  .well-known/jwks.json  -> 200
#   NVIDIA RIM service (A-25 driver RIM)GET  NV_GPU_DRIVER_GH100_.. -> 200
#   NVIDIA OCSP (local nvtrust only)    GET  /                      -> any HTTP answer (reachability only)
set -u
fail=0
probe() { # name method url expected
    local name="$1" method="$2" url="$3" want="$4" code
    if [ "$method" = POST ]; then
        code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 20 -X POST -H 'Content-Type: application/json' -d '{}' "$url")
    else
        code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 20 "$url")
    fi
    if [ "$want" = any ] && [ "$code" != 000 ]; then ok=PASS
    elif [ "$code" = "$want" ]; then ok=PASS
    else ok=FAIL; fail=1; fi
    printf '%-4s %-38s %s (want %s)  %s\n' "$ok" "$name" "$code" "$want" "$url"
}
echo "A-18 broker egress from $(hostname) at $(date -u +%FT%TZ)"
probe "PCCS rootcacrl (Phala)"        GET  'https://pccs.phala.network/sgx/certification/v4/rootcacrl' 200
probe "Intel PCS tdx qe/identity"     GET  'https://api.trustedservices.intel.com/tdx/certification/v4/qe/identity' 200
probe "Intel SGX root CA"             GET  'https://certificates.trustedservices.intel.com/IntelSGXRootCA.der' 200
probe "NRAS v3 attest/gpu (empty)"    POST 'https://nras.attestation.nvidia.com/v3/attest/gpu' 400
probe "NRAS JWKS"                     GET  'https://nras.attestation.nvidia.com/.well-known/jwks.json' 200
probe "NVIDIA RIM driver 580.95.05"   GET  'https://rim.attestation.nvidia.com/v1/rim/NV_GPU_DRIVER_GH100_580.95.05' 200
probe "NVIDIA OCSP (reachability)"    GET  'https://ocsp.ndis.nvidia.com/' any
[ "$fail" = 0 ] && echo "A-18: all reachable" || echo "A-18: FAILED (see above)"
exit $fail

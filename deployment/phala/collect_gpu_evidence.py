# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
"""Collect NVIDIA GPU attestation evidence for the key broker (Phase 5, node side).

Usage: python3 collect_gpu_evidence.py <nonce_hex_32_bytes>
Prints one JSON object on stdout in the shape Phala's reference node ships
(Dstack-TEE/vllm-proxy quote.py `_build_nvidia_payload`):

    {"nonce": "<hex>", "evidence_list": [{"certificate", "evidence", "arch"}], "arch": "HOPPER"}

The node collects; it never verifies. The broker POSTs this payload to NRAS.

TEE_GPU_EVIDENCE=canned returns nvtrust's `test_no_gpu` sample evidence, which
carries nvtrust's FIXED nonce inside the report (BaseSettings.NONCE), not ours.
The payload is labelled "canned": true so no broker can mistake it for real
evidence; only a broker started with KBS_GPU_EVIDENCE=canned (test keyring)
accepts it. Any other value of TEE_GPU_EVIDENCE than unset/"real" is an error.
"""
import json
import os
import sys

# nvtrust's `info_log` is a logging.StreamHandler bound to sys.stdout at import
# time (verifier/config.py), and collect_gpu_evidence() logs "Number of GPUs
# available", "Fetching GPU 0 ..." and "All GPU Evidences fetched successfully"
# through it in real AND canned mode. Those lines would land in front of our
# JSON and the node would reject the payload as not-JSON on every attestation.
# So: everything nvtrust prints goes to stderr, and the payload alone goes to
# the ORIGINAL stdout, kept here before any import can bind to it. (Found by the
# P2 converge review, 2026-09-17.)
PAYLOAD_OUT = sys.stdout
sys.stdout = sys.stderr

NONCE_BYTES = 32


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: collect_gpu_evidence.py <nonce_hex>", file=sys.stderr)
        return 64
    nonce_hex = sys.argv[1].lower().removeprefix("0x")
    try:
        if len(bytes.fromhex(nonce_hex)) != NONCE_BYTES:
            raise ValueError
    except ValueError:
        print("nonce must be 32 bytes of hex", file=sys.stderr)
        return 64

    mode = os.environ.get("TEE_GPU_EVIDENCE", "real")
    if mode not in ("real", "canned"):
        print(f"TEE_GPU_EVIDENCE must be real|canned, got {mode!r}", file=sys.stderr)
        return 78
    canned = mode == "canned"

    # Imported late so a missing nvtrust fails here, with a clear message, not
    # at the top of an unrelated stack.
    from verifier import cc_admin  # nv-local-gpu-verifier

    if not canned:
        # Read the three host-side states BEFORE collecting, and print them, so
        # the day-one log says why collection failed instead of a stack trace.
        # PPCIe (multi-GPU protected PCIe): nvtrust's single-GPU path raises
        # "Attestation in standalone mode is not supported for PPCIE system"
        # when it is on (cc_admin.init_nvml). The multi-GPU alternative (SDK,
        # NVSwitch evidence, NRAS /attest/switch) is not built, so fail closed
        # here with the exact reason rather than let nvtrust raise (gate B-1a,
        # known gap G-14). DevTools is printed because the signed report may
        # not carry it (G-6); this line is the node-asserted reading.
        from verifier.nvml import NvmlHandler

        NvmlHandler.init_nvml()
        cc_on = NvmlHandler.is_cc_enabled()
        ppcie = NvmlHandler.is_ppcie_mode_enabled()
        devtools = NvmlHandler.is_cc_dev_mode()
        print(
            f"gpu-state: cc_enabled={cc_on} ppcie={ppcie} devtools={devtools}",
            file=sys.stderr,
        )
        if not cc_on:
            print("GPU confidential computing is OFF; refusing to collect evidence", file=sys.stderr)
            return 75
        if ppcie:
            print(
                "PPCIe (multi-GPU protected PCIe) is ON: nvtrust's single-GPU collection "
                "path cannot attest a PPCIe system and the multi-GPU path is not built. "
                "Failing closed (gate B-1a, gap G-14).",
                file=sys.stderr,
            )
            return 75

    evidence_list = cc_admin.collect_gpu_evidence_remote(nonce_hex, no_gpu_mode=canned)
    if not evidence_list:
        print("no GPU evidence collected", file=sys.stderr)
        return 1
    # The top-level arch is what NRAS judges the whole payload under; take it from
    # the evidence nvtrust produced (GPU_ARCHITECTURE_MAP: HOPPER, BLACKWELL, ...)
    # rather than a constant, and refuse a mixed list rather than guess.
    arches = {e.get("arch") for e in evidence_list}
    if len(arches) != 1 or None in arches:
        print(f"evidence entries disagree on arch or lack it: {sorted(map(str, arches))}", file=sys.stderr)
        return 1
    arch = arches.pop()
    payload = {"nonce": nonce_hex, "evidence_list": evidence_list, "arch": arch}
    if canned:
        payload["canned"] = True
    # One line, the only thing ever written to the real stdout.
    PAYLOAD_OUT.write(json.dumps(payload, separators=(",", ":")) + "\n")
    PAYLOAD_OUT.flush()
    return 0


if __name__ == "__main__":
    sys.exit(main())

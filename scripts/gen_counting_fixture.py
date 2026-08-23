#!/usr/bin/env python3
"""Generate the per-template counting fixture for Training M0 (interface C.2, count-v1).

Usage (GPU host, where the pinned base's tokenizer.json lives):

    pip install tokenizers            # the HF tokenizers library — the SAME family the
                                      # trainer sidecar uses, per TD3 (zero drift)
    python3 scripts/gen_counting_fixture.py \
        --tokenizer /path/to/tokenizer.json \
        --specials-per-sample 1 \
        --out tests/training/vectors/counting-fixture.json

count-v1 recipe (frozen, interface C.2): tokens(sample) =
len(encode(text, add_special_tokens=False).ids) + specialsPerSample.
The fixture's tokenizerSha256 is the SHA256 of the exact tokenizer.json bytes and must
equal the training template's pin. The fixture's canonical home is
tests/training/vectors/counting-fixture.json (one home; the template dir may reference,
not duplicate, it). Both sides implement counting strictly from these cases.
"""
import argparse
import hashlib
import json
import sys

# >= 12 cases per the interface: ASCII, Unicode incl. emoji + CJK, whitespace runs,
# long sample, one-char sample. Deliberately includes shapes where tokenizer
# implementations historically diverge (leading space, NFC AND NFD accent forms, mixed
# scripts, digits, newlines-in-text). The NFC/NFD PAIR is load-bearing: whether the
# pinned tokenizer's normalizer runs identically on both sides is exactly what
# cross-implementation counting drift looks like (round-2 converge finding).
CASES = [
    "hello world",
    "a",
    "The quick brown fox jumps over the lazy dog.",
    " leading space matters to byte-level BPE",
    "trailing space matters too ",
    "double  space   and\ttab and\nnewline runs",
    "emoji: \U0001F680\U0001F9EA\U0001F3AC and skin tone \U0001F44D\U0001F3FD",
    "中文分词测试：训练任务在链上结算。",
    "日本語と한국어 mixed with English and 12345 digits",
    "café naïve résumé — accented forms (NFC precomposed)",
    "café naïve résumé — accented forms (NFD combining marks)",
    "fn main() { println!(\"{}\", 42_u64.pow(2)); } // code-ish sample",
    ("A deliberately long sample intended to cross several merge boundaries. " * 40).strip(),
    "0123456789 " * 12,
    "?!.,;:'\"()[]{}<>|/\\@#$%^&*-_=+~`",
]


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--tokenizer", required=True, help="path to the pinned tokenizer.json")
    p.add_argument("--specials-per-sample", type=int, default=1,
                   help="template constant specialsPerSample (M0 template: 1)")
    p.add_argument("--out", required=True, help="output fixture path")
    args = p.parse_args()

    try:
        import tokenizers
        from tokenizers import Tokenizer
    except ImportError:
        print("ERROR: pip install tokenizers", file=sys.stderr)
        return 1

    with open(args.tokenizer, "rb") as f:
        raw = f.read()
    tokenizer_sha256 = "0x" + hashlib.sha256(raw).hexdigest()

    tok = Tokenizer.from_file(args.tokenizer)
    cases = []
    for text in CASES:
        ids = tok.encode(text, add_special_tokens=False).ids
        cases.append({"text": text, "tokens": len(ids) + args.specials_per_sample})

    fixture = {
        "_note": ("count-v1 (interface C.2): tokens = len(encode(text, "
                  "add_special_tokens=False)) + specialsPerSample. tokenizerSha256 must "
                  "equal the training template's pin. generator records the exact "
                  "tokenizers library version the counts came from (Open item 7 parity)."),
        "countingRecipe": "count-v1",
        "tokenizerSha256": tokenizer_sha256,
        "specialsPerSample": args.specials_per_sample,
        "generator": {"library": "tokenizers", "version": tokenizers.__version__},
        "cases": cases,
    }
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(fixture, f, ensure_ascii=False, indent=2)
        f.write("\n")
    print(f"wrote {args.out}: {len(cases)} cases, tokenizerSha256={tokenizer_sha256}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

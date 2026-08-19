#!/usr/bin/env python3
# v1.19.0 — Merges-table generator for axon-csys BPE kernel.
#
# Produces deterministic, little-endian binary serialisations of the
# `cl100k_base` and `o200k_base` merge tables drawn directly from the
# canonical `tiktoken` Python package (the same source-of-truth that
# `tiktoken-rs` was sourcing from). The resulting `.bin` artefacts are
# embedded into `c-src/tokens/bpe.c` via C23 `#embed` (with an
# `include_bytes!` shim fallback when the toolchain lacks `#embed`).
#
# Run once when adopting a new tiktoken release; commit the output.
# Adopters never run this — they consume the committed `.bin` files.
#
# Format (little-endian, no alignment, append-only versionable):
#
#   offset  bytes  field
#   ──────  ─────  ─────
#   0       4      magic "AXBP" (0x50 0x42 0x58 0x41)
#   4       4      version (u32) — currently 1
#   8       4      vocab_size (u32) — number of (bytes, rank) entries
#   12      4      regex_pat_len (u32) — UTF-8 byte length
#   16      N      regex_pat (UTF-8) — pretokenizer pattern
#   16+N    4      entries_byte_count (u32) — sanity check
#   20+N    …      entries: each = [u8 len][len bytes][u32 LE rank]
#
# The regex pattern is embedded for self-describing tables, but the
# Rust shim uses its own copy (compiled into the binary) for
# pretokenisation — the embedded copy is for cross-validation in the
# drift gate. Token byte length is u8 (max in tiktoken cl100k+o200k is
# 128, fits in u8 if 0 means "256-byte token" → we don't have those, so
# u8 0..255 covers everything).

from __future__ import annotations

import struct
import sys
from pathlib import Path

import tiktoken

MAGIC = b"AXBP"
VERSION = 1


def serialise(name: str) -> bytes:
    """Build the deterministic binary blob for the named tiktoken encoding."""
    enc = tiktoken.get_encoding(name)
    ranks: dict[bytes, int] = enc._mergeable_ranks
    pat: str = enc._pat_str
    pat_bytes = pat.encode("utf-8")

    # Sort by rank ascending — the order is observable for downstream
    # consumers that walk the table in rank order (e.g. priority-queue
    # construction).
    items = sorted(ranks.items(), key=lambda kv: kv[1])

    body_parts: list[bytes] = []
    for token_bytes, rank in items:
        if not 1 <= len(token_bytes) <= 255:
            raise ValueError(
                f"{name} token {token_bytes!r} length {len(token_bytes)} out of u8 range"
            )
        body_parts.append(struct.pack("<B", len(token_bytes)))
        body_parts.append(token_bytes)
        body_parts.append(struct.pack("<I", rank))
    body = b"".join(body_parts)

    header = (
        MAGIC
        + struct.pack("<I", VERSION)
        + struct.pack("<I", len(items))
        + struct.pack("<I", len(pat_bytes))
        + pat_bytes
        + struct.pack("<I", len(body))
    )
    return header + body


def main() -> int:
    out_dir = Path(__file__).parent.parent / "c-src" / "tokens"
    out_dir.mkdir(parents=True, exist_ok=True)

    for name in ("cl100k_base", "o200k_base"):
        blob = serialise(name)
        path = out_dir / f"merges_{name}.bin"
        path.write_bytes(blob)
        print(f"wrote {path} ({len(blob):,} bytes)")

    return 0


if __name__ == "__main__":
    sys.exit(main())

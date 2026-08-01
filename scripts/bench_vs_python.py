#!/usr/bin/env python3
"""Compare Rust skiff vs upstream Python mcp2cli (multi-run → dataframe).

Requires:
  - CF_API_TOKEN or CLOUDFLARE_API_TOKEN (or a project .env with either)
  - Rust binary: SKIFF_BIN or ./target/release/skiff
  - Python CLI: SKIFF_PYTHON_BIN or `uvx mcp2cli`
  - pandas (`pip install pandas` / `uv pip install pandas`)

Example:
  python3 scripts/bench_vs_python.py --runs 10
  python3 scripts/bench_vs_python.py --runs 5 --csv /tmp/bench.csv
"""

from __future__ import annotations

import argparse
import math
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
DOCS_URL = "https://docs.mcp.cloudflare.com/mcp"
FAT_URL = "https://mcp.cloudflare.com/mcp?codemode=false"


def load_dotenv(path: Path) -> None:
    if not path.is_file():
        return
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, _, val = line.partition("=")
        key = key.strip()
        val = val.strip().strip("'").strip('"')
        os.environ.setdefault(key, val)


def token() -> str:
    t = os.environ.get("CF_API_TOKEN") or os.environ.get("CLOUDFLARE_API_TOKEN")
    if not t:
        sys.exit("Set CF_API_TOKEN or CLOUDFLARE_API_TOKEN (or add to .env)")
    return t


def auth_header() -> str:
    """Return an auth header that keeps the token off argv.

    skiff rejects literal ``--auth-header`` values; both skiff and Python mcp2cli
    accept ``env:`` / ``file:`` prefixes. Prefer ``Authorization:Bearer:env:…``
    so the process list never contains the raw token.
    """
    # Ensure the token is present for env: resolution (also loads from .env above).
    _ = token()
    # Prefer CF_API_TOKEN; fall back so either env name works for env:VAR.
    if not os.environ.get("CF_API_TOKEN") and os.environ.get("CLOUDFLARE_API_TOKEN"):
        os.environ["CF_API_TOKEN"] = os.environ["CLOUDFLARE_API_TOKEN"]
    return "Authorization:Bearer:env:CF_API_TOKEN"


def resolve_rust_bin() -> str:
    env = os.environ.get("SKIFF_BIN") or os.environ.get("SKIFF_RUST_BIN")
    if env:
        return env
    cand = ROOT / "target" / "release" / "skiff"
    if cand.is_file():
        return str(cand)
    print("Building release skiff…", file=sys.stderr)
    subprocess.run(
        ["cargo", "build", "--release"],
        cwd=ROOT,
        check=True,
        env={**os.environ, "CARGO_TARGET_DIR": str(ROOT / "target")},
    )
    if not cand.is_file():
        sys.exit(f"missing rust binary at {cand}")
    return str(cand)


def resolve_python_bin() -> list[str]:
    env = os.environ.get("SKIFF_PYTHON_BIN")
    if env:
        return env.split()
    if shutil.which("uvx"):
        # Plain `uvx mcp2cli` can pull an mcp SDK that renamed streamablehttp_client.
        # Pin a known-good transport for fair HTTP streamable benches.
        return ["uvx", "--with", "mcp==1.12.0", "mcp2cli"]
    sys.exit("Set SKIFF_PYTHON_BIN or install uv (`uvx mcp2cli`)")


def approx_tokens(n_bytes: int) -> int:
    return math.ceil(n_bytes / 4) if n_bytes else 0


def run_once(
    argv: list[str],
    *,
    cache_dir: Path,
    timeout: float = 180.0,
) -> dict[str, Any]:
    env = {**os.environ, "SKIFF_CACHE_DIR": str(cache_dir)}
    # Avoid agent env leaking into fair comparisons.
    env.pop("SKIFF_AGENT", None)
    t0 = time.perf_counter()
    try:
        proc = subprocess.run(
            argv,
            capture_output=True,
            env=env,
            timeout=timeout,
        )
        ms = (time.perf_counter() - t0) * 1000.0
        out = proc.stdout or b""
        err = (proc.stderr or b"").decode("utf-8", errors="replace")[:400]
        return {
            "ok": proc.returncode == 0,
            "exit": proc.returncode,
            "ms": ms,
            "bytes": len(out),
            "tokens": approx_tokens(len(out)),
            "stderr": err,
        }
    except subprocess.TimeoutExpired:
        return {
            "ok": False,
            "exit": -1,
            "ms": timeout * 1000.0,
            "bytes": 0,
            "tokens": 0,
            "stderr": "timeout",
        }


def index_sizes(cache_dir: Path) -> list[tuple[str, int]]:
    out: list[tuple[str, int]] = []
    for p in sorted(cache_dir.glob("*_tools_index.json")):
        out.append((p.name, p.stat().st_size))
    for p in sorted(cache_dir.glob("*_index.json")):
        out.append((p.name, p.stat().st_size))
    return out


def main() -> int:
    load_dotenv(ROOT / ".env")
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--runs", type=int, default=10, help="warm repeats per scenario")
    ap.add_argument("--csv", type=Path, default=None, help="write raw rows CSV")
    ap.add_argument("--skip-python", action="store_true")
    ap.add_argument("--skip-session", action="store_true")
    args = ap.parse_args()

    try:
        import pandas as pd
    except ImportError:
        sys.exit("pandas required: pip install pandas  (or uv pip install pandas)")

    rust = resolve_rust_bin()
    py = None if args.skip_python else resolve_python_bin()
    auth = auth_header()

    scenarios: list[dict[str, Any]] = [
        {
            "id": "docs_list_compact",
            "url": DOCS_URL,
            "flags": ["--list", "--json", "--compact"],
        },
        {
            "id": "docs_list_json",
            "url": DOCS_URL,
            "flags": ["--list", "--json"],
        },
        {
            "id": "fat_search_workers",
            "url": FAT_URL,
            "flags": ["--search", "workers", "--json", "--compact", "--top", "20"],
        },
        {
            "id": "fat_list_compact",
            "url": FAT_URL,
            "flags": ["--list", "--json", "--compact"],
        },
    ]

    rows: list[dict[str, Any]] = []
    rust_cache = Path(tempfile.mkdtemp(prefix="skiff-rust-bench-"))
    py_cache = Path(tempfile.mkdtemp(prefix="skiff-py-bench-"))

    print(f"rust={rust}")
    print(f"python={' '.join(py) if py else '(skipped)'}")
    print(f"runs={args.runs}  rust_cache={rust_cache}  py_cache={py_cache}")
    print()

    impls: list[tuple[str, list[str], Path]] = [("rust", [rust], rust_cache)]
    if py:
        impls.append(("python", py, py_cache))

    for scen in scenarios:
        for impl_name, bin_prefix, cache in impls:
            base = [
                *bin_prefix,
                "--mcp",
                scen["url"],
                "--transport",
                "streamable",
                "--auth-header",
                auth,
                *scen["flags"],
            ]
            # Cold
            cold_argv = [*base, "--refresh"]
            cold = run_once(cold_argv, cache_dir=cache)
            rows.append(
                {
                    "scenario": scen["id"],
                    "impl": impl_name,
                    "phase": "cold",
                    "run": 0,
                    **{k: cold[k] for k in ("ok", "exit", "ms", "bytes", "tokens")},
                    "stderr": cold["stderr"],
                }
            )
            label = f"{scen['id']}/{impl_name}/cold"
            status = "ok" if cold["ok"] else f"FAIL exit={cold['exit']}"
            print(
                f"{label:<42} {status:<16} {cold['ms']:8.1f} ms  "
                f"bytes={cold['bytes']:<8} ~tok={cold['tokens']}"
            )
            if not cold["ok"]:
                print(f"  stderr: {cold['stderr'][:200]}")

            # Warm
            for i in range(1, args.runs + 1):
                warm = run_once(base, cache_dir=cache)
                rows.append(
                    {
                        "scenario": scen["id"],
                        "impl": impl_name,
                        "phase": "warm",
                        "run": i,
                        **{k: warm[k] for k in ("ok", "exit", "ms", "bytes", "tokens")},
                        "stderr": warm["stderr"],
                    }
                )
                if not warm["ok"]:
                    print(
                        f"{scen['id']}/{impl_name}/warm#{i} FAIL: {warm['stderr'][:160]}"
                    )

    # Rust-only session search path
    if not args.skip_session:
        sess_cache = Path(tempfile.mkdtemp(prefix="skiff-sess-bench-"))
        sess_name = "benchfat"
        start = run_once(
            [
                rust,
                "--mcp",
                FAT_URL,
                "--transport",
                "streamable",
                "--auth-header",
                auth,
                "--session-start",
                sess_name,
                "--session-idle-secs",
                "600",
            ],
            cache_dir=sess_cache,
            timeout=240.0,
        )
        rows.append(
            {
                "scenario": "rust_session_start",
                "impl": "rust",
                "phase": "cold",
                "run": 0,
                **{k: start[k] for k in ("ok", "exit", "ms", "bytes", "tokens")},
                "stderr": start["stderr"],
            }
        )
        print(
            f"{'rust_session_start':<42} "
            f"{'ok' if start['ok'] else 'FAIL':<16} {start['ms']:8.1f} ms"
        )
        if start["ok"]:
            # Prime index in daemon
            _ = run_once(
                [
                    rust,
                    "--session",
                    sess_name,
                    "--agent",
                    "--search",
                    "workers",
                ],
                cache_dir=sess_cache,
            )
            for i in range(1, args.runs + 1):
                warm = run_once(
                    [
                        rust,
                        "--session",
                        sess_name,
                        "--agent",
                        "--search",
                        "workers",
                    ],
                    cache_dir=sess_cache,
                )
                rows.append(
                    {
                        "scenario": "rust_session_search",
                        "impl": "rust",
                        "phase": "warm",
                        "run": i,
                        **{k: warm[k] for k in ("ok", "exit", "ms", "bytes", "tokens")},
                        "stderr": warm["stderr"],
                    }
                )
            _ = run_once(
                [rust, "--session-stop", sess_name],
                cache_dir=sess_cache,
            )
        else:
            print(f"  stderr: {start['stderr'][:300]}")

    df = pd.DataFrame(rows)
    if args.csv:
        df.to_csv(args.csv, index=False)
        print(f"\nWrote {args.csv}")

    print("\n=== Per-scenario summary (ok runs only) ===")
    ok = df[df["ok"]].copy()
    if ok.empty:
        print("No successful runs.")
        return 1

    summary_rows = []
    for (scenario, impl, phase), g in ok.groupby(["scenario", "impl", "phase"]):
        summary_rows.append(
            {
                "scenario": scenario,
                "impl": impl,
                "phase": phase,
                "n": len(g),
                "ms_mean": g["ms"].mean(),
                "ms_median": g["ms"].median(),
                "ms_p90": g["ms"].quantile(0.9),
                "bytes_median": g["bytes"].median(),
                "tokens_median": g["tokens"].median(),
            }
        )
    summary = pd.DataFrame(summary_rows).sort_values(["scenario", "impl", "phase"])
    with pd.option_context("display.max_rows", 200, "display.width", 120):
        print(summary.to_string(index=False, float_format=lambda x: f"{x:.1f}"))

    print("\n=== Rust disk index sizes (after cold fetches) ===")
    sizes = index_sizes(rust_cache)
    if not sizes:
        print("(no *_tools_index.json found)")
    for name, sz in sizes:
        print(f"  {name}: {sz} bytes ({sz / 1024:.1f} KiB)")

    print("\n=== Net-better check (fat_search_workers, warm) ===")
    rust_w = ok[
        (ok.scenario == "fat_search_workers") & (ok.impl == "rust") & (ok.phase == "warm")
    ]
    py_w = ok[
        (ok.scenario == "fat_search_workers")
        & (ok.impl == "python")
        & (ok.phase == "warm")
    ]
    if rust_w.empty:
        print("Rust warm fat search missing — cannot compare.")
        return 1
    r_ms = float(rust_w["ms"].median())
    r_b = float(rust_w["bytes"].median())
    print(f"Rust  median warm: {r_ms:.1f} ms, {r_b:.0f} bytes (~{approx_tokens(int(r_b))} tok)")
    if py_w.empty:
        print("Python warm fat search missing (skipped or failed).")
    else:
        p_ms = float(py_w["ms"].median())
        p_b = float(py_w["bytes"].median())
        print(
            f"Python median warm: {p_ms:.1f} ms, {p_b:.0f} bytes (~{approx_tokens(int(p_b))} tok)"
        )
        speedup = p_ms / r_ms if r_ms > 0 else float("inf")
        byte_ratio = p_b / r_b if r_b > 0 else float("inf")
        better_speed = r_ms <= p_ms
        better_bytes = r_b <= p_b
        net = better_speed and better_bytes
        print(f"Speedup (py/rust ms): {speedup:.2f}x")
        print(f"Byte ratio (py/rust): {byte_ratio:.2f}x")
        print(
            f"Net better (Rust ≤ Python on median warm ms AND bytes): "
            f"{'YES' if net else 'NO'}"
        )
        if not net:
            print(
                f"  speed_ok={better_speed} bytes_ok={better_bytes}",
                file=sys.stderr,
            )

    sess = ok[(ok.scenario == "rust_session_search") & (ok.phase == "warm")]
    if not sess.empty:
        print(
            f"\nRust session warm search median: {sess['ms'].median():.1f} ms, "
            f"{sess['bytes'].median():.0f} bytes"
        )

    # Cleanup temp caches (best-effort)
    for d in (rust_cache, py_cache):
        shutil.rmtree(d, ignore_errors=True)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

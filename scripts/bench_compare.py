#!/usr/bin/env python3
"""
bench_compare.py — compare two benchmark result JSON files and fail if any
metric has regressed beyond the allowed threshold.

Usage:
    python scripts/bench_compare.py \\
        --baseline  bench_results_baseline.json \\
        --current   bench_results_current.json  \\
        --threshold 20.0

Exit codes:
    0  — no regressions (or no baseline to compare against)
    1  — one or more metrics regressed beyond threshold
    2  — bad arguments / unreadable files

Result JSON schema (produced by bench_extract.py):
    {
      "commit": "abc123",
      "timestamp": "2026-09-25T12:00:00Z",
      "benchmarks": {
        "<test_name>": {
          "cpu": 1234567
        }
      }
    }
"""

import argparse
import json
import sys


def load(path: str) -> dict:
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, json.JSONDecodeError) as exc:
        print(f"ERROR: cannot load {path}: {exc}", file=sys.stderr)
        sys.exit(2)


def compare(baseline: dict, current: dict, threshold: float) -> bool:
    """Return True if all metrics are within threshold, False if any regressed."""
    base_bench = baseline.get("benchmarks", {})
    curr_bench = current.get("benchmarks", {})

    if not base_bench:
        print("Baseline contains no benchmarks — skipping comparison.")
        return True

    regressions = []
    improvements = []
    unchanged = []

    all_keys = sorted(set(base_bench) | set(curr_bench))

    for name in all_keys:
        if name not in base_bench:
            print(f"  NEW     {name}: cpu={curr_bench[name]['cpu']:,}  (no baseline)")
            continue
        if name not in curr_bench:
            print(f"  MISSING {name}: was cpu={base_bench[name]['cpu']:,}  (not in current run)")
            continue

        base_cpu = base_bench[name]["cpu"]
        curr_cpu = curr_bench[name]["cpu"]

        if base_cpu == 0:
            print(f"  SKIP    {name}: baseline cpu=0, skipping")
            continue

        delta_pct = (curr_cpu - base_cpu) / base_cpu * 100.0

        if delta_pct > threshold:
            regressions.append((name, base_cpu, curr_cpu, delta_pct))
        elif delta_pct < -5.0:
            improvements.append((name, base_cpu, curr_cpu, delta_pct))
        else:
            unchanged.append((name, base_cpu, curr_cpu, delta_pct))

    # ── Print summary ──────────────────────────────────────────────────────────
    print(f"\nBaseline commit : {baseline.get('commit', 'unknown')}")
    print(f"Current  commit : {current.get('commit', 'unknown')}")
    print(f"Regression threshold: {threshold:.1f}%\n")

    col = "{:<55} {:>14} {:>14} {:>10}"
    print(col.format("Benchmark", "Baseline CPU", "Current CPU", "Delta"))
    print("-" * 97)

    for name, b, c, d in improvements:
        print(col.format(name, f"{b:,}", f"{c:,}", f"{d:+.1f}%  ✓"))
    for name, b, c, d in unchanged:
        print(col.format(name, f"{b:,}", f"{c:,}", f"{d:+.1f}%"))
    for name, b, c, d in regressions:
        print(col.format(name, f"{b:,}", f"{c:,}", f"{d:+.1f}%  ✗ REGRESSION"))

    print()

    if regressions:
        print(
            f"FAIL: {len(regressions)} benchmark(s) regressed by more than "
            f"{threshold:.0f}%:"
        )
        for name, _, curr_cpu, delta_pct in regressions:
            print(f"  • {name}: +{delta_pct:.1f}% (cpu={curr_cpu:,})")
        print(
            "\nIf the regression is intentional (e.g. a new feature adds storage "
            "writes), update the baseline by merging the current results file and "
            "documenting the rationale in the PR description."
        )
        return False

    print(f"PASS: all {len(unchanged) + len(improvements)} benchmarks within {threshold:.0f}% threshold.")
    return True


def main() -> None:
    parser = argparse.ArgumentParser(description="Compare benchmark results.")
    parser.add_argument("--baseline", required=True, help="Path to baseline JSON")
    parser.add_argument("--current", required=True, help="Path to current JSON")
    parser.add_argument(
        "--threshold",
        type=float,
        default=20.0,
        help="Regression threshold in percent (default: 20.0)",
    )
    args = parser.parse_args()

    baseline = load(args.baseline)
    current = load(args.current)

    ok = compare(baseline, current, args.threshold)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()

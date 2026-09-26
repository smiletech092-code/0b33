#!/usr/bin/env python3
"""
bench_extract.py — parse `cargo test -- --nocapture` output and emit a
structured JSON results file for storage and comparison.

Lines produced by the benchmark harnesses look like:

    [bench_charge_cold_warm] standard_active: Cold CPU=1123456  ...
    [bench_batch_charge] 100_all_chargeable: measured_cpu=4567890  baseline_cpu=0
    [bench_batch_charge] scaling: per_item@100=45678  per_item@500=46123  ratio=1.010
    [bench_dispute_lifecycle] standard::open  CPU=456789  ...
    [bench_withdraw_fixed_cost] standard: measured=1234567 baseline=1200000 ...
    [bench_ring_position_cost] head=234567 mid=234890 tail=235012 absent=235678
    [bench_batch_charge_scaling] Success,100,4567890,45678

The extractor recognises the `CPU=` / `cpu=` / `measured_cpu=` / `measured=`
patterns that every bench harness already prints and maps them to a simple
  { "<label>": { "cpu": <int> } }
dictionary keyed by a slug built from the bench name and scenario.

Usage:
    cargo test -p subscription_vault -- --nocapture 2>&1 | \\
        python scripts/bench_extract.py --commit abc123 > bench_results.json

    # Or from a saved log file:
    python scripts/bench_extract.py --input test_output.txt --commit abc123 \\
        > bench_results.json
"""

import argparse
import datetime
import json
import re
import sys


# ---------------------------------------------------------------------------
# Patterns
# ---------------------------------------------------------------------------

# Matches lines like:
#   [bench_foo] scenario: ... CPU=1234567 ...
#   [bench_foo] scenario: ... cpu=1234567 ...
#   [bench_foo] scenario: measured_cpu=1234567 ...
#   [bench_foo] scenario: measured=1234567 ...
#   [bench_foo] scenario: Cold CPU=1234567 ...
_BENCH_LINE = re.compile(
    r"\[(?P<bench>[^\]]+)\]\s+"
    r"(?P<scenario>[^:]+):\s+"
    r".*?(?:measured_cpu|measured|[Cc]old\s+CPU|CPU|cpu)=(?P<cpu>\d+)",
    re.IGNORECASE,
)

# Matches the scaling CSV line: Scenario,Size,CPU_Cost,Cost_Per_Item
_CSV_LINE = re.compile(
    r"^(?P<scenario>\w+),(?P<size>\d+),(?P<cpu>\d+),(?P<per_item>\d+)$"
)


def slugify(bench: str, scenario: str) -> str:
    """Build a stable dict key from bench name and scenario description."""
    raw = f"{bench.strip()}__{scenario.strip()}"
    return re.sub(r"[^a-zA-Z0-9_@]", "_", raw).lower()


def extract(lines: list[str]) -> dict:
    results: dict[str, dict] = {}

    for line in lines:
        line = line.rstrip()

        # Try the main tagged-line pattern first.
        m = _BENCH_LINE.match(line)
        if m:
            key = slugify(m.group("bench"), m.group("scenario"))
            cpu = int(m.group("cpu"))
            # Keep the largest CPU value if the same key appears twice
            # (e.g. cold and warm are on the same output line — we take cold).
            if key not in results or cpu > results[key]["cpu"]:
                results[key] = {"cpu": cpu}
            continue

        # Try the CSV scaling line (no bench tag prefix).
        m2 = _CSV_LINE.match(line)
        if m2:
            key = slugify(
                "bench_batch_charge_scaling",
                f"{m2.group('scenario')}_{m2.group('size')}",
            )
            results[key] = {"cpu": int(m2.group("cpu"))}

    return results


def main() -> None:
    parser = argparse.ArgumentParser(description="Extract bench metrics from test output.")
    parser.add_argument("--input", default="-", help="Input file (default: stdin)")
    parser.add_argument("--commit", default="unknown", help="Git commit SHA")
    args = parser.parse_args()

    if args.input == "-":
        raw = sys.stdin.read()
    else:
        with open(args.input, encoding="utf-8") as f:
            raw = f.read()

    lines = raw.splitlines()
    benchmarks = extract(lines)

    output = {
        "commit": args.commit,
        "timestamp": datetime.datetime.utcnow().isoformat() + "Z",
        "benchmarks": benchmarks,
    }

    print(json.dumps(output, indent=2))

    # Print a brief summary to stderr so it appears in the CI log.
    print(
        f"bench_extract: captured {len(benchmarks)} benchmark metrics",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()

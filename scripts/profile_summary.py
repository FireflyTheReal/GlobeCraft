"""Summarize opt-in timings, retaining raster dimensions and phase boundaries."""
import argparse
import csv
import json
import math
import statistics
from collections import defaultdict
from pathlib import Path


SLOW_TRACE_OPERATION = "ui.slow"


def slow_passes(rows):
    """Group the watchdog's summary and section events by UI pass."""
    passes = {}
    for row in rows:
        operation = row["operation"]
        is_slow_trace = operation == SLOW_TRACE_OPERATION or operation.startswith(
            f"{SLOW_TRACE_OPERATION}."
        )
        if not is_slow_trace:
            continue
        key = row["unix_us"]
        entry = passes.setdefault(key, {
            "unix_us": int(key),
            "total_ms": None,
            "viewport": None,
            "sections": [],
        })
        if operation == SLOW_TRACE_OPERATION:
            entry["total_ms"] = round(int(row["elapsed_us"]) / 1000, 3)
            entry["viewport"] = [int(row["width"]), int(row["height"])]
        else:
            entry["sections"].append({
                "section": operation.removeprefix(f"{SLOW_TRACE_OPERATION}."),
                "ms": round(int(row["elapsed_us"]) / 1000, 3),
            })
    for entry in passes.values():
        entry["sections"].sort(key=lambda section: section["ms"], reverse=True)
    return sorted(passes.values(), key=lambda entry: entry["unix_us"])


def summarize(rows):
    groups = defaultdict(list)
    for row in rows:
        operation = row["operation"]
        if operation == SLOW_TRACE_OPERATION or operation.startswith(
            f"{SLOW_TRACE_OPERATION}."
        ):
            continue
        key = (operation, int(row["width"]), int(row["height"]))
        groups[key].append(int(row["elapsed_us"]) / 1000)
    timings = []
    for (operation, width, height), values in sorted(groups.items()):
        values.sort()
        timings.append({
            "operation": operation,
            "width": width,
            "height": height,
            "samples": len(values),
            "median_ms": round(statistics.median(values), 3),
            "p95_ms": round(values[max(0, math.ceil(len(values) * 0.95) - 1)], 3),
            "max_ms": round(values[-1], 3),
        })
    # `frame.raster` puts camera altitude in resident_bytes, so it cannot
    # contribute to the decoded-cache estimate.
    decoded = (int(row["resident_bytes"]) for row in rows if row["operation"] != "frame.raster")
    return {
        "timings": timings,
        "slow_passes": slow_passes(rows),
        "max_inflight": max((int(row["inflight"]) for row in rows), default=0),
        "max_decoded_estimate_mib": round(max(decoded, default=0) / 1048576, 2),
        "dropped_events_counter": max((int(row["dropped_events"]) for row in rows), default=0),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capture", type=Path)
    parser.add_argument("--phases", type=Path, help="JSON array: phase, start_us, end_us")
    args = parser.parse_args()
    with args.capture.open(newline="") as source:
        rows = [row for row in csv.DictReader(source) if all(row.get(key) for key in ("unix_us", "operation", "elapsed_us", "width", "height", "inflight", "resident_bytes", "dropped_events"))]
    phases = json.loads(args.phases.read_text()) if args.phases else [{"phase": "all", "start_us": 0, "end_us": 2**64}]
    report = {}
    for phase in phases:
        selected = [row for row in rows if phase["start_us"] <= int(row["unix_us"]) < phase.get("end_us", 2**64)]
        report[phase["phase"]] = summarize(selected)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()

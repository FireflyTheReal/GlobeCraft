"""Checks for the opt-in profiling summary."""
import unittest

import profile_summary


def row(operation, elapsed_us, *, unix_us=1, width=800, height=600, resident_bytes=0):
    return {
        "unix_us": str(unix_us),
        "operation": operation,
        "elapsed_us": str(elapsed_us),
        "width": str(width),
        "height": str(height),
        "inflight": "0",
        "resident_bytes": str(resident_bytes),
        "count": "0",
        "dropped_events": "0",
    }


class SlowPassSummaryTest(unittest.TestCase):
    def test_sections_are_grouped_with_the_summary_and_sorted_by_cost(self):
        rows = [
            row("ui.update", 60_000),
            row("ui.slow", 60_000),
            row("ui.slow.header", 4_000),
            row("ui.slow.globe", 20_000),
            row("ui.slow.panes", 5_000),
        ]
        report = profile_summary.summarize(rows)
        self.assertEqual([timing["operation"] for timing in report["timings"]], ["ui.update"])
        self.assertEqual(len(report["slow_passes"]), 1)
        slow = report["slow_passes"][0]
        self.assertEqual(slow["total_ms"], 60.0)
        self.assertEqual(slow["viewport"], [800, 600])
        self.assertEqual(
            [(section["section"], section["ms"]) for section in slow["sections"]],
            [("globe", 20.0), ("panes", 5.0), ("header", 4.0)],
        )

    def test_raster_altitude_does_not_pollute_the_decoded_cache_estimate(self):
        rows = [
            row("frame.raster", 10_000, resident_bytes=10_000_000),
            row("ui.update", 1_000, resident_bytes=2 * 1024 * 1024),
        ]
        report = profile_summary.summarize(rows)
        self.assertEqual(report["max_decoded_estimate_mib"], 2.0)


if __name__ == "__main__":
    unittest.main()

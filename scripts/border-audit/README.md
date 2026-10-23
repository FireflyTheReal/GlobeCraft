# Border audit

Runs GlobeCraft's own municipal border code on every land border whose two countries
load geoBoundaries files, and measures what the app would draw. Use it before and after
any change to `boundaries/municipal`, `topology` or `overlays/handover`: a fix must
improve the totals without making other borders worse.

## Run

```sh
DATA=~/gb-data                                   # any folder with room for ~600 MB
# Natural Earth 1:10m into $DATA: ne_10m_admin_0_boundary_lines_land.geojson,
# ne_10m_admin_1_states_provinces_lines.geojson (github.com/nvkelso/natural-earth-vector)
python3 scripts/border-audit/make_pairs.py $DATA   # 283 pairs on 2026-10-23
python3 scripts/border-audit/fetch_all.py $DATA    # geoBoundaries files, ~570 MB
cargo test --release -p globecraft-data -p globecraft-render --no-run
# the two `unittests src/lib.rs` binaries cargo printed:
python3 scripts/border-audit/audit_run.py $DATA <data test binary> <render test binary> $DATA/audit/after.jsonl
python3 scripts/border-audit/audit_summary.py $DATA/audit/after.jsonl
python3 scripts/border-audit/audit_compare.py before.jsonl after.jsonl
```

A full run takes about 40 minutes on two cores. To compare with an older version, build
it in its own worktree **and its own `CARGO_TARGET_DIR`** (the test binaries have the
same names, and a shared target folder silently reuses the other version's build).

## What is measured

Each pair's Natural Earth line is sampled every kilometre inside the audited area:

| Class | Meaning |
| --- | --- |
| replaced | The coarse line is gone and a municipal country line runs within 15 km |
| lost | The coarse line is gone and nothing replaces it |
| double | The coarse line stays while a municipal country line runs within 15 km: two lines |
| coarse_data | The coarse line stays and municipal outlines exist nearby, but none was taken as this border |
| coarse_nodata | The coarse line stays; no municipal data nearby |

Plus **breaks** (ends of drawn lines that meet nothing, away from coasts, tripoints and
the area's edge) and **stray** (municipal country lines over 10 km from any Natural
Earth border; where both countries' outlines agree, this is usually Natural Earth being
off).

`diag.py <check.json> <ne lines> A B out.png lat,lon,radius` plots one spot: the full
Natural Earth line (yellow dashed), what stays of it (white), municipal country lines
(pink) and the samples of each class.

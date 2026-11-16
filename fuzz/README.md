# Fuzzing the decoders

GlobeCraft reads files and server answers it does not control: KML and KMZ, GeoJSON,
GPX and CSV, PMTiles and MBTiles archives, vector and image tiles, JSON feeds, gzip
files and its own settings. A fuzzer feeds each decoder millions of damaged inputs
and reports any that make it panic, overflow, hang or allocate without limit.

The targets use [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) (libFuzzer).
This crate is its own workspace, so a normal `cargo build` or `cargo test` never
compiles it.

## Running

```sh
rustup toolchain install nightly
cargo install cargo-fuzz --locked

fuzz/run.sh             # every target, 60 seconds each
fuzz/run.sh 600 kml     # one target for ten minutes
```

Linux and macOS. `run.sh` prints the executed inputs per target, or the panic and the
saved input of a finding, and exits with 1 if there was one. New inputs collect in
`fuzz/corpus/<target>/` (not committed), so a later run continues where this one
stopped.

By hand:

```sh
cd fuzz
cargo +nightly fuzz run kml corpus/kml seeds/kml -- -max_total_time=60
```

## Targets

| Target | Decoder |
| --- | --- |
| `kml`, `kmz` | KML documents; KMZ archives (zip) |
| `features` | A user layer's GeoJSON, GPX or CSV |
| `wms_capabilities` | WMS and WMTS capabilities (XML) |
| `pmtiles` | PMTiles header, directories and tile ids |
| `mbtiles_rows` | An MBTiles database with arbitrary rows |
| `vector_tile` | Mapbox vector tiles: borders, place names, roads, transport, places of interest |
| `compact_tile` | The border cache's stored tiles |
| `image_tile` | Imagery, Terrarium elevation and layer images (PNG, JPEG, WebP) |
| `planet` | Planet files and the sky image |
| `json_feeds` | Aircraft, vessels, satellites, aurora, time zones, sea names, search, Wayback, the map-data manifest |
| `compressed` | Every gzip reader |
| `settings` | Exported settings and data-source files |
| `text_inputs` | View links, typed coordinates, dates, web addresses, colours, the airport table |
| `assistant_reply` | Model providers' answers and streamed events |

The entry points are in `crates/globecraft-data/src/fuzzing.rs` (behind the data
crate's `fuzzing` feature); a target in `fuzz_targets/` only passes the bytes on.
Besides "does not crash", several check a property: a stored tile reads back as it was
written, settings survive an export and import, decoded coordinates are in range.

## Seeds

`seeds/<target>/` holds small valid inputs the fuzzer starts from, and every input
that once found a defect (`found-*`), so each run replays them first. The valid ones
are written by `python3 fuzz/make_seeds.py` (needs Pillow); run it only to change
them.

## When a run finds something

1. The input is saved as `fuzz/artifacts/<target>/crash-…` (or `oom-…`, `timeout-…`).
   Replay it: `cargo +nightly fuzz run <target> fuzz/artifacts/<target>/crash-…`.
2. Fix the decoder, and add a unit test next to it with the smallest input that
   shows the defect: the test suite, not the fuzzer, keeps it fixed.
3. Copy the saved input to `seeds/<target>/found-<kind>-<first 8 characters>`.

## Adding a target

Add an entry point to `fuzzing.rs`, a file in `fuzz_targets/`, a `[[bin]]` in
`Cargo.toml`, seeds in `make_seeds.py`, and a row in the table above.

## Limits

`run.sh` gives each input 10 seconds and 256 MiB for a single allocation (600 MiB for
`planet`: a planet's picture may be 16 384 × 8192 pixels). Inputs are at most 64 KiB:
enough for every format's structure, and a decoder that needs more than that to
misbehave is caught by its size limit instead.

#![no_main]
//! The rows of an MBTiles file's `tiles` table, with any integers SQLite can hold.
use globecraft_data::fuzzing::MbtilesRow;

#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
    rows: Vec<(i64, i64, i64, Vec<u8>)>,
    ask: (u8, u32, u32),
}

libfuzzer_sys::fuzz_target!(|input: Input| {
    let rows: Vec<MbtilesRow> = input
        .rows
        .into_iter()
        .take(16)
        .map(|(zoom, column, row, data)| MbtilesRow {
            zoom,
            column,
            row,
            data,
        })
        .collect();
    globecraft_data::fuzzing::mbtiles_rows(&rows, input.ask);
});

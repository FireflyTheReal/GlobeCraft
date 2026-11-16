# Settings read through serde (review A5)

**Status:** implemented 2026-11-15. Checked against the previous reader on 13,894
settings files: the same result for every one.

## Why this item

Each setting was declared four times: the field, its default, and two hand-written
readers (`value.get("…").and_then(|v| v.as_bool())` and the assignment). A setting whose
reader was forgotten was saved but never read back, silently.

## What changed

`crates/globecraft-data/src/settings/lenient.rs` reads a struct's plain fields through
serde. A section derives `Serialize` and `Deserialize` (`#[serde(default)]`), and its
`apply` starts with `lenient::apply(self, value, &[])`:

- the section as it stands is serialized, the file's values are put over it key by
  key, and the result is read back. Nearly always that is one read;
- if a value does not read (the wrong type, an unknown name of an enum), the fields
  are tried one at a time and each that reads is kept. A bad value costs only its own
  field, and that field keeps the value it had, as before.

So a new setting is a field and its default. Ranges are one `clamp` after the read.

What stays explicit, because a type cannot say the rule:

- sections and tables (anything saved as an object): the struct that holds one calls
  its `apply`. This keeps "apply onto what is there" at every depth;
- fields named in `own_rules`: values valid only together (the startup latitude and
  longitude), lists filtered entry by entry (ribbon tools, user layers), names with
  older spellings that are not plain aliases (the ribbon tab), an empty folder name;
- the cache sizes, where any whole number is taken and brought into range.

Fields saved under a short name read back with `lenient::coded`; tables with a reader
of their own (map style, layer opacities, the coordinate display) with
`lenient::applied`.

## The invariant

The reader starts from what the settings save, so settings must read back exactly what
they save, including states only a running application has. The test
`settings_read_back_exactly_what_they_save` holds that; `lenient::apply` asserts it in
debug builds.

## How it was checked

- The existing tests, including the v0.3.0 and v0.4.0 fixtures.
- A comparison of the old and new readers (both built as a small program printing the
  exported settings): 5,308 files from the `settings` fuzz corpus and seeds, and 8,586
  generated files that put one odd value (`null`, a boolean, numbers in and out of
  range, text, lists, objects) at each of the 163 places of a full settings file, onto
  that file and alone, plus each key removed. All 13,894 read identically.

## Not changed

The file format, the version number and `migrate`, the fixtures, and what any file
reads as.

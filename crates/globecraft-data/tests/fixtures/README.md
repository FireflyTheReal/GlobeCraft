# Data fixtures

- `settings-v0.3.0.json`: a settings file written by the v0.3.0 release code
  (tag `v0.3.0`, `Settings::default()` with a customised theme, navigator, scroll
  direction, cache size, sidebar, folders, text sizes, line style and layers,
  serialised with `serde_json::to_string_pretty`). The test
  `settings_written_by_v0_3_0_keep_every_value` requires every value in it to
  survive loading and saving with the current version. Do not regenerate it with
  newer code: it stands for files users already have.
- `settings-v0.4.0.json`: a settings file written by the v0.4.0 release code
  (tag `v0.4.0`): `Settings::default()` with a JSON patch customising the sky,
  theme, navigation and start view, cache, sidebar, workspace panes, layers and
  terrain, assistant, folders, scales, rendering switches, units, line style,
  label sizes and the experience level, applied with `apply_json` and serialised
  with `serde_json::to_string_pretty` (values v0.4.0 refused stayed default).
  Checked by `settings_written_by_v0_4_0_keep_every_value`; do not regenerate.

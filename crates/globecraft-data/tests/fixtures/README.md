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
- `settings-v0.5.0.json`: a settings file written by the v0.5.0 release code
  (tag `v0.5.0`): `Settings::default()` with `settings-v0.4.0.json` as a JSON patch
  plus the settings added in v0.5.0 customised (reduced motion, monochrome icons and
  their colour, compact ribbon, pane side, moved ribbon tools, quick tools, flat live
  objects, cloud resolution, layer opacities, the time-zone line and label size,
  offline, UTC times, keys in the keychain, the GPU globe off, HD render quality,
  motion resolutions, smooth relief off), applied with `apply_json` and serialised
  with `serde_json::to_string_pretty` (the left pane's values moved into the side
  pane, as v0.5.0 reads them). Checked by
  `settings_written_by_v0_5_0_keep_every_value`; do not regenerate.

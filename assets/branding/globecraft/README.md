# GlobeCraft brand kit

GlobeCraft has two logos. Both are Rolf Böhm's **Bohème Star** projection, computed from
Natural Earth's 1:110m physical land, downloaded from the official site:

| Mark  | Projection | Used for |
|-------|------------|----------|
| **Bloom** | Bohème Star, 5 tips, 15° N boundary | Logo: README, About window, favicon, wordmark lockups, social |
| **Limit** | Bohème Star, tips → ∞ (the whole Earth in one disc, Antarctica as its edge) | App icon on macOS, Windows and Linux |

## Where the logo is used

| Place | File |
|-------|------|
| macOS app bundle and Dock | `assets/packaging/macos/icon.icns`, `assets/packaging/macos/icon.png` |
| Windows installer, exe, taskbar | `assets/packaging/icon.ico` |
| Linux window icon, DEB, RPM, AppImage, Flatpak, Snap | `assets/packaging/icon_256.png` (master: `assets/packaging/icon.png`) |
| About window and About command icon | `logo/globecraft-bloom-mask-{256,64}.png`, white masks the app tints with the theme's ink (`apps/globecraft/src/app/ui/brand.rs`) |
| README | `logo/globecraft-lockup-horizontal-{light,dark}.svg`, chosen by GitHub's theme |
| AppStream (Flathub, GNOME Software) | brand colours in `packaging/globecraft.metainfo.xml` |
| Website | `web/favicon.svg` (follows the browser theme), `web/favicon.ico`, `web/apple-touch-icon.png` |
| macOS 26 Icon Composer | `app-icon/AppIcon-{light,dark}-1024.png` |
| GitHub social preview | `social/github-social-preview-light.png` (upload in Settings → Social preview) |

## Regenerate

```sh
pip install shapely cairosvg pillow
python scripts/generate_logos.py          # SVG marks, app-icon SVGs, favicon.svg
python scripts/generate_logos.py --png    # also every packaging icon and PNG above
```

The script downloads `ne_110m_land.zip` from Natural Earth's official server
(`naciscdn.org`), checks it against a pinned SHA-256 and reads the shapefile itself; it uses
nothing else from this repository. Pass `--data ne_110m_land.zip` to work offline from a copy
you downloaded yourself.
Change colours or geometry in the script and re-run; don't edit the exported files by hand.
The wordmark lockups (`logo/globecraft-lockup-*`, `logo/globecraft-wordmark-*`) and `social/`
are finished artwork built on the same Bloom mark.

## Colours: Slate

| Token | Light | Dark | Role |
|-------|-------|------|------|
| `paper` | `#e9edf1` | `#11171e` | tile, land, background |
| `ink`   | `#2f4459` | `#b4c3d2` | ocean, wordmark |

Ink on paper has a contrast of 8.5 : 1 (light) and 10.0 : 1 (dark), both above WCAG AAA (7 : 1).

## Rules

- Clear space round the logo is **¼ of the Bloom's height** on every side.
- Minimum size: Bloom **16 px**; horizontal lockup **120 px** wide. Below that, use the Bloom alone.
- Use the light logo on light backgrounds and the dark one on dark backgrounds. No other colours.
- Never rotate or mirror the marks. The −16° central meridian (Africa and Europe facing down, the
  Americas left) is fixed.
- Don't add outlines, shadows, glows or rims. The only shading is the built-in ocean depth.
- Don't redraw the land. If a size needs less detail, the script's small-size optics handle it
  (rasters of 32 px and below use slightly simplified shapes).

## Packaging notes

- macOS: Apple's 1024 grid, an 824 px tile with 22.5 % corners and a 100 px margin.
- Windows and Linux: the globe is the whole icon, a full circle with no tile and no border.

## Website snippet

```html
<link rel="icon" href="/favicon.svg" type="image/svg+xml">
<link rel="icon" href="/favicon.ico" sizes="16x16 32x32 48x48">
<link rel="apple-touch-icon" href="/apple-touch-icon.png">
<meta property="og:image" content="https://…/og-image-light.png">
<meta name="theme-color" content="#e9edf1" media="(prefers-color-scheme: light)">
<meta name="theme-color" content="#11171e" media="(prefers-color-scheme: dark)">
```

Wordmark: Manrope ExtraBold (SIL Open Font License), converted to outlines.
Projection: Bohème Star by Dr.-Ing. Rolf Böhm, boehmwanderkarten.de. Land: Natural Earth, public domain.

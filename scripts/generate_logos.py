"""Generate the GlobeCraft logos and app icons from Natural Earth land data.

Both logos are Rolf Böhm's Bohème Star projection (boehmwanderkarten.de):
  Bloom  5 tips, boundary parallel 15° N
  Limit  infinitely many tips: the whole sphere as one disc, the south pole as its rim

Colours are Slate, two-tone: paper fills the tile and the land, ink fills the ocean.

Land comes from the official Natural Earth 1:110m physical land download
(naciscdn.org, public domain). The script fetches it on each run, checks it against a
pinned SHA-256 and reads the shapefile itself, so it depends on nothing in this repository.
Pass --data to use a zip you downloaded yourself.

Writes to assets/branding/globecraft:
  logo/globecraft-{bloom,limit}-mark-{light,dark}.svg   transparent single-colour marks
  app-icon/globecraft-app-{mac,round}-{light,dark}.svg
  web/favicon.svg
With --png (needs cairosvg and pillow) also:
  assets/packaging/icon.png, icon_256.png, icon.ico          Windows and Linux (round)
  assets/packaging/macos/icon.png, macos/icon.icns           macOS (Apple grid tile)
  assets/branding/globecraft/app-icon/AppIcon-{light,dark}-1024.png   macOS 26 Icon Composer
  assets/branding/globecraft/logo/globecraft-bloom-mask-{256,64}.png   white masks the app tints
                                                     (About window and About command icon)
  assets/branding/globecraft/web/favicon.ico, apple-touch-icon.png

Requires shapely >= 2.
"""
import argparse
import hashlib
import io
import math
import struct
import urllib.request
import zipfile
from pathlib import Path

import numpy as np
from shapely import segmentize, transform
from shapely.affinity import translate
from shapely.geometry import GeometryCollection, LinearRing, MultiPolygon, Point, Polygon, box
from shapely.ops import unary_union

ROOT = Path(__file__).resolve().parents[1]
LAND_URL = "https://naciscdn.org/naturalearth/110m/physical/ne_110m_land.zip"
LAND_SHA256 = "1926c621afd6ac67c3f36639bb1236134a48d82226dc675d3e3df53d02d2a3de"  # Natural Earth 5.1.x
BRAND = ROOT / "assets/branding/globecraft"
PACKAGING = ROOT / "assets/packaging"

SLATE = {"light": ("#e9edf1", "#2f4459"), "dark": ("#11171e", "#b4c3d2")}  # (paper, ink)
CENTRAL = -16.0     # central meridian: Africa and Europe face down, the Americas left
TIPS, BOUNDARY = 5, 15.0
ICE_LAT = -60.0     # Antarctica is drawn as ice (paper) south of this
DEPTH = 0.2         # ocean vignette
SIZE = 1024
# Generalisation in plane units (radians of polar distance). "regular" is used for
# rasters of 32 px and below so small icons stay crisp.
DETAIL = {"fine": dict(speck=0.0015, soft=0.007, boost=0.012, ice=0.012),
          "regular": dict(speck=0.008, soft=0.022, boost=0.020, ice=0.025)}


# ---------------------------------------------------------------- geometry helpers
def polygons(g):
    if isinstance(g, Polygon):
        yield g
    elif isinstance(g, (MultiPolygon, GeometryCollection)):
        for part in g.geoms:
            yield from polygons(part)


def drop_specks(g, area):
    return unary_union([p for p in polygons(g) if p.area >= area])


def soften(g, r):
    return g.buffer(r, 16).buffer(-2 * r, 16).buffer(r, 16)


def generalise(g, d):
    g = unary_union([p.buffer(d["boost"] if p.area < 0.03 else 0, 16) for p in polygons(g)])
    return soften(drop_specks(g, d["speck"]), d["soft"])


def fetch_land(path=None):
    """The official Natural Earth 1:110m land zip, from --data or downloaded."""
    if path:
        data = Path(path).read_bytes()
    else:
        request = urllib.request.Request(LAND_URL, headers={"User-Agent": "GlobeCraft-logo-script"})
        with urllib.request.urlopen(request, timeout=60) as response:
            data = response.read()
    digest = hashlib.sha256(data).hexdigest()
    if digest != LAND_SHA256:
        raise ValueError(f"Natural Earth land zip has SHA-256 {digest}, expected {LAND_SHA256}. "
                         "If Natural Earth published a new version, check it and update LAND_SHA256.")
    return data


def read_shapefile_polygons(zip_bytes):
    """Polygon records of ne_110m_land.shp (ESRI shapefile type 5), as shapely geometry."""
    with zipfile.ZipFile(io.BytesIO(zip_bytes)) as archive:
        shp = archive.read("ne_110m_land.shp")
    outers, holes, offset = [], [], 100          # skip the 100-byte file header
    while offset < len(shp):
        _, words = struct.unpack(">ii", shp[offset:offset + 8])
        record = shp[offset + 8:offset + 8 + 2 * words]
        offset += 8 + 2 * words
        if struct.unpack("<i", record[:4])[0] != 5:
            continue
        parts, points = struct.unpack("<ii", record[36:44])
        starts = list(struct.unpack(f"<{parts}i", record[44:44 + 4 * parts])) + [points]
        xy = struct.unpack(f"<{2 * points}d", record[44 + 4 * parts:44 + 4 * parts + 16 * points])
        for i, j in zip(starts, starts[1:]):
            ring = LinearRing([(xy[2 * k], xy[2 * k + 1]) for k in range(i, j)])
            # shapefile outer rings run clockwise, holes counter-clockwise
            (holes if ring.is_ccw else outers).append(Polygon(ring).buffer(0))
    return list(polygons(unary_union(outers).difference(unary_union(holes))))


def load_land(zip_bytes):
    # rotate to the central meridian; keep copies either side so clipping never loses land
    return [translate(g, xoff=s - CENTRAL) for g in read_shapefile_polygons(zip_bytes) for s in (-360, 0, 360)]


def project(geoms, clip, fn):
    out = []
    for g in geoms:
        for p in polygons(g.intersection(clip)):
            out.append(transform(segmentize(p, 0.4), lambda xy: np.array([fn(x, y) for x, y in xy])).buffer(0))
    return unary_union(out)


# ---------------------------------------------------------------- the projection
def north(lon, lat):
    """North of the boundary: azimuthal equidistant, lambda + 270° so 0° points down."""
    m, a = math.pi / 2 - math.radians(lat), math.radians(lon + 270)
    return m * math.cos(a), m * math.sin(a)


def south(lon, lat, k):
    """South of the boundary: petal k of Böhm's RTA formula."""
    w = 360 / TIPS
    local = lon - (-180 + k * w)
    phi = math.radians(lat)
    r0 = (math.pi / 2 + phi) / (1 + math.radians(BOUNDARY) / (math.pi / 2))
    a = math.radians(local) * math.sin(r0) + math.radians(90 + k * w)
    m = math.pi / 2 - phi
    return m * math.cos(a), m * math.sin(a)


def bloom(geoms, d):
    """Ocean of the 5-tip star: star minus land minus Antarctic ice."""
    w = 360 / TIPS
    land = [g.intersection(box(-540, ICE_LAT, 540, 90)) for g in geoms]
    ice = [g.intersection(box(-540, -90, 540, ICE_LAT + 0.01)) for g in geoms]
    star = [Point(0, 0).buffer(math.pi / 2 - math.radians(BOUNDARY), 256)]
    parts = {"land": [project(land, box(-180, BOUNDARY, 180, 90), north)], "ice": []}
    for k in range(TIPS):
        lo, hi = -180 + k * w - w / 2, -180 + k * w + w / 2
        fn = lambda x, y, k=k: south(x, y, k)
        clip = box(lo, -90, hi, BOUNDARY + 1e-9)
        parts["land"].append(project(land, clip, fn))
        parts["ice"].append(project(ice, clip, fn))
        b = BOUNDARY + 0.5
        ring = ([(lo + i * (hi - lo) / 240, b) for i in range(241)]
                + [(hi, b - i * (b + 90) / 240) for i in range(1, 241)]
                + [(lo, -90 + i * (b + 90) / 240) for i in range(1, 241)])
        star.append(Polygon([fn(x, y) for x, y in ring]).buffer(0))
    star = unary_union(star)
    land = generalise(unary_union(parts["land"]).intersection(star), d)
    ice = generalise(unary_union(parts["ice"]).intersection(star), d)
    ice = drop_specks(ice.buffer(-d["ice"], 16).buffer(d["ice"], 16).intersection(star), d["speck"])
    return star.difference(land).difference(ice), math.pi, 0.03   # ocean, outer radius, y offset


def limit(geoms, d):
    """Ocean of the star with infinitely many tips: the azimuthal disc out to the south pole."""
    disc = Point(0, 0).buffer(math.pi, 512)
    whole = box(-180, -89.999, 180, 90)
    land = project([g.intersection(box(-540, ICE_LAT, 540, 90)) for g in geoms], whole, north)
    ice = project([g.intersection(box(-540, -90, 540, ICE_LAT + 0.01)) for g in geoms], whole, north)
    # a first light smoothing pass, then the shared generalisation
    land, ice = (soften(drop_specks(g, 0.002), 0.01) for g in (land, ice))
    land, ice = generalise(land, d), generalise(ice, d)
    return disc.difference(land).difference(ice), math.pi, 0.0


# ---------------------------------------------------------------- SVG
def path_d(g, cx, cy, s):
    out = []
    for p in polygons(g.simplify(0.0012, preserve_topology=True)):
        for ring in (p.exterior, *p.interiors):
            pts = [(cx + x * s, cy - y * s) for x, y in ring.coords[:-1]]
            if len(pts) > 2:
                out.append("M" + "L".join(f"{x:.2f} {y:.2f}" for x, y in pts) + "Z")
    return "".join(out)


def svg(body, w=SIZE, h=SIZE, defs=""):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}">'
            f'<title>GlobeCraft</title><defs>{defs}</defs>{body}</svg>\n')


def mark(logo, mode, size=SIZE, colour=None):
    """Transparent single-colour mark: ink ocean, land cut out."""
    ocean, R, dy = logo
    r = size / 2 / (1.02 if dy else 1.0)   # the star's tips get a 2 % margin, the disc fills the canvas
    fill = colour or SLATE[mode][1]
    return svg(f'<path d="{path_d(ocean, size / 2, size / 2 + dy * r, r / R)}" fill="{fill}"/>', size, size)


def app_icon(logo, mode, platform):
    """macOS: Apple's 1024 grid, 824 px tile with 22.5 % corners. round: the globe is the icon."""
    ocean, R, _ = logo
    paper, ink = SLATE[mode]
    if platform == "mac":
        tile, r = f'<rect x="100" y="100" width="824" height="824" rx="185.4" fill="{paper}"/>', 824 * 0.46
    else:
        tile, r = f'<circle cx="512" cy="512" r="512" fill="{paper}"/>', 512
    d = path_d(ocean, 512, 512, r / R)
    grad = (f'<radialGradient id="dg" gradientUnits="userSpaceOnUse" cx="512" cy="{512 - r * 0.13:.1f}" r="{r * 1.05:.1f}">'
            '<stop offset=".35" stop-color="#000" stop-opacity="0"/><stop offset="1" stop-color="#000"/></radialGradient>')
    return svg(tile + f'<path d="{d}" fill="{ink}"/><path d="{d}" fill="url(#dg)" opacity="{DEPTH}"/>', defs=grad)


def favicon(logo):
    ocean, R, dy = logo
    r = 31.4
    (pl, il), (pd, idk) = SLATE["light"], SLATE["dark"]
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><style>path{{fill:{il}}}'
            f'@media (prefers-color-scheme:dark){{path{{fill:{idk}}}}}</style>'
            f'<path d="{path_d(ocean, 32, 32 + dy * r, r / R)}"/></svg>\n')


# ---------------------------------------------------------------- output
def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def rasters(logos, small):
    import cairosvg
    from PIL import Image

    def png(text, px):
        data = cairosvg.svg2png(bytestring=text.encode(), output_width=px, output_height=px)
        return Image.open(io.BytesIO(data)).convert("RGBA")

    def icon(mode, platform, px):
        return png(app_icon(small["limit"] if px <= 32 else logos["limit"], mode, platform), px)

    # Windows and Linux: the round globe. icon_256.png is the runtime window icon there.
    icon("light", "round", 1024).save(PACKAGING / "icon.png")
    icon("light", "round", 256).save(PACKAGING / "icon_256.png")
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
    imgs = [icon("light", "round", s) for s in sizes]
    imgs[-1].save(PACKAGING / "icon.ico", sizes=[(s, s) for s in sizes], append_images=imgs[:-1])
    # macOS: Apple grid tile with transparent Dock padding, 16-1024 px in the .icns
    icon("light", "mac", 1024).save(PACKAGING / "macos/icon.png")
    icon("light", "mac", 1024).save(PACKAGING / "macos/icon.icns",
                                    append_images=[icon("light", "mac", s) for s in (16, 32, 64, 128, 256, 512)])
    for mode in SLATE:
        icon(mode, "mac", 1024).save(BRAND / f"app-icon/AppIcon-{mode}-1024.png")
    # white masks: the app tints them with the theme's ink (the small one uses small-size optics)
    png(mark(logos["bloom"], "light", colour="#ffffff"), 256).save(BRAND / "logo/globecraft-bloom-mask-256.png")
    png(mark(small["bloom"], "light", colour="#ffffff"), 64).save(BRAND / "logo/globecraft-bloom-mask-64.png")
    fav = [png(mark(small["bloom"] if s <= 32 else logos["bloom"], "light", 64), s) for s in (16, 32, 48)]
    fav[-1].save(BRAND / "web/favicon.ico", sizes=[(16, 16), (32, 32), (48, 48)], append_images=fav[:-1])
    paper, ink = SLATE["light"]
    ocean, R, _ = logos["limit"]
    touch = svg(f'<rect width="180" height="180" fill="{paper}"/><path d="{path_d(ocean, 90, 90, 72 / R)}" fill="{ink}"/>', 180, 180)
    png(touch, 180).save(BRAND / "web/apple-touch-icon.png")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--png", action="store_true", help="also write the packaging icons and PNG/ICO files")
    parser.add_argument("--data", help="path to ne_110m_land.zip instead of downloading it")
    args = parser.parse_args()
    geoms = load_land(fetch_land(args.data))
    logos = {"bloom": bloom(geoms, DETAIL["fine"]), "limit": limit(geoms, DETAIL["fine"])}
    for name, logo in logos.items():
        for mode in SLATE:
            write(BRAND / f"logo/globecraft-{name}-mark-{mode}.svg", mark(logo, mode))
    for mode in SLATE:
        for platform in ("mac", "round"):
            write(BRAND / f"app-icon/globecraft-app-{platform}-{mode}.svg", app_icon(logos["limit"], mode, platform))
    write(BRAND / "web/favicon.svg", favicon(logos["bloom"]))
    if args.png:
        small = {"bloom": bloom(geoms, DETAIL["regular"]), "limit": limit(geoms, DETAIL["regular"])}
        rasters(logos, small)
    print(f"Wrote GlobeCraft logos to {BRAND.relative_to(ROOT)}" + (" and icons to assets/packaging" if args.png else ""))


if __name__ == "__main__":
    main()

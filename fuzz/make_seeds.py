#!/usr/bin/env python3
"""Writes the small valid inputs the fuzz targets start from, into fuzz/seeds/<target>/.

The seeds are committed; run this again only to change them (needs Pillow for the
JPEG and WebP images). Each fuzzer mutates these, so a seed only has to reach the
decoder's interesting code, not be realistic.
"""
import gzip
import io
import json
import struct
import zipfile
import zlib
from pathlib import Path

ROOT = Path(__file__).parent / "seeds"


def write(target: str, name: str, data) -> None:
    folder = ROOT / target
    folder.mkdir(parents=True, exist_ok=True)
    (folder / name).write_bytes(data.encode() if isinstance(data, str) else data)


def varint(value: int) -> bytes:
    out = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        out.append(byte | (0x80 if value else 0))
        if not value:
            return bytes(out)


def field(number: int, wire: int, payload: bytes) -> bytes:
    key = varint(number << 3 | wire)
    return key + (varint(len(payload)) + payload if wire == 2 else payload)


def zigzag(value: int) -> int:
    return (value << 1) ^ (value >> 31)


def geometry(commands) -> bytes:
    """Mapbox vector tile geometry: (command id, [(dx, dy), ...]) pairs."""
    out = b""
    for command, points in commands:
        out += varint(command | max(len(points), 1) << 3)
        for dx, dy in points:
            out += varint(zigzag(dx)) + varint(zigzag(dy))
    return out


def mvt_layer(name: str, features, extent: int = 4096) -> bytes:
    """features: (geometry type, {key: value}, geometry bytes)."""
    keys, values, body = [], [], b""
    for kind, tags, geom in features:
        tag_ids = b""
        for key, value in tags.items():
            if key not in keys:
                keys.append(key)
            if value not in values:
                values.append(value)
            tag_ids += varint(keys.index(key)) + varint(values.index(value))
        feature = field(2, 2, tag_ids) + field(3, 0, varint(kind)) + field(4, 2, geom)
        body += field(2, 2, feature)
    for key in keys:
        body += field(3, 2, key.encode())
    for value in values:
        if isinstance(value, str):
            encoded = field(1, 2, value.encode())
        elif isinstance(value, float):
            encoded = field(3, 1, struct.pack("<d", value))
        else:
            encoded = field(4, 0, varint(value))
        body += field(4, 2, encoded)
    layer = field(15, 0, varint(2)) + field(1, 2, name.encode()) + body
    return field(3, 2, layer + field(5, 0, varint(extent)))


def vector_tile() -> bytes:
    point = geometry([(1, [(2048, 2048)])])
    line = geometry([(1, [(100, 100)]), (2, [(900, 300), (700, 1200)])])
    ring = geometry([(1, [(500, 500)]), (2, [(1000, 0), (0, 1000), (-1000, 0)]), (7, [])])
    return b"".join(
        [
            mvt_layer(
                "place",
                [(1, {"name": "Lindau", "name:en": "Lindau", "class": "town", "rank": 9}, point)],
            ),
            mvt_layer(
                "boundary",
                [(2, {"admin_level": 4, "maritime": 0, "disputed": 0}, line)],
            ),
            mvt_layer("water", [(3, {"class": "lake"}, ring)]),
            mvt_layer(
                "transportation",
                [
                    (2, {"class": "motorway", "ramp": 0}, line),
                    (2, {"class": "rail", "subclass": "rail"}, line),
                ],
            ),
            mvt_layer(
                "transportation_name",
                [(2, {"name": "Seestraße", "ref": "A96", "class": "motorway"}, line)],
            ),
            mvt_layer(
                "poi",
                [(1, {"name": "Hafen", "class": "harbor", "subclass": "marina", "rank": 3}, point)],
            ),
            mvt_layer(
                "aerodrome_label",
                [(1, {"name": "Flughafen", "iata": "FDH", "class": "regional"}, point)],
            ),
            mvt_layer("mountain_peak", [(1, {"name": "Pfänder", "ele": 1064}, point)]),
            mvt_layer("water_name", [(1, {"name": "Bodensee", "class": "lake"}, point)]),
        ]
    )


def tile_key(z: int, x: int, y: int) -> bytes:
    return bytes([z]) + struct.pack("<II", x, y)


def png(width: int, height: int, colour=(40, 120, 200, 255)) -> bytes:
    def chunk(kind: bytes, data: bytes) -> bytes:
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    row = b"\x00" + bytes(colour) * width
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(row * height, 9))
        + chunk(b"IEND", b"")
    )


def pil_image(fmt: str) -> bytes:
    from PIL import Image

    out = io.BytesIO()
    Image.new("RGB", (16, 16), (90, 140, 60)).save(out, fmt)
    return out.getvalue()


def pmtiles(compression: int) -> bytes:
    tile = png(4, 4)
    entries = [(0, 0, len(tile), 1), (1, len(tile), len(tile), 4)]
    directory = varint(len(entries))
    last = 0
    for tile_id, *_ in entries:
        directory += varint(tile_id - last)
        last = tile_id
    for *_, run in entries:
        directory += varint(run)
    for _, _, length, _ in entries:
        directory += varint(length)
    for _, offset, _, _ in entries:
        directory += varint(offset + 1)
    stored = gzip.compress(directory) if compression == 2 else directory
    header = bytearray(127)
    header[:7] = b"PMTiles"
    header[7] = 3
    root_offset = 127
    struct.pack_into("<QQ", header, 8, root_offset, len(stored))
    struct.pack_into("<QQ", header, 40, root_offset + len(stored), 0)
    struct.pack_into("<QQ", header, 56, root_offset + len(stored), 2 * len(tile))
    header[97], header[98], header[99] = compression, 1, 2
    header[100], header[101] = 0, 1
    struct.pack_into("<iiii", header, 102, -1_800_000_000, -850_000_000, 1_800_000_000, 850_000_000)
    header[118] = 0
    struct.pack_into("<ii", header, 119, 95_000_000, 475_000_000)
    return bytes(header) + stored + tile + tile


KML = """<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2"><Document><name>Trip</name>
<Style id="pin"><IconStyle><color>ff0000ff</color><scale>1.2</scale></IconStyle></Style>
<Folder><name>Lake</name><open>1</open>
<Placemark><name>Lindau</name><description><![CDATA[<b>Harbour</b>]]></description>
<styleUrl>#pin</styleUrl><LookAt><longitude>9.68</longitude><latitude>47.54</latitude>
<range>5000</range><tilt>30</tilt><heading>10</heading></LookAt>
<Point><coordinates>9.6833,47.5458,400</coordinates></Point></Placemark>
<Placemark><name>Route</name><LineString><coordinates>9.6,47.5 9.7,47.6 9.8,47.55</coordinates>
</LineString></Placemark>
<Placemark><name>Area</name><Polygon><outerBoundaryIs><LinearRing><coordinates>
9,47 10,47 10,48 9,48 9,47</coordinates></LinearRing></outerBoundaryIs></Polygon></Placemark>
</Folder></Document></kml>
"""

GEOJSON = {
    "type": "FeatureCollection",
    "features": [
        {
            "type": "Feature",
            "properties": {"name": "Quake", "mag": 5.2, "time": 1794400000000, "place": "Sea"},
            "geometry": {"type": "Point", "coordinates": [142.3, 38.1, -10]},
        },
        {
            "type": "Feature",
            "properties": {"title": "Track"},
            "geometry": {"type": "LineString", "coordinates": [[9.6, 47.5], [9.7, 47.6]]},
        },
        {
            "type": "Feature",
            "properties": None,
            "geometry": {
                "type": "MultiPolygon",
                "coordinates": [[[[0, 0], [1, 0], [1, 1], [0, 0]]]],
            },
        },
        {
            "type": "Feature",
            "geometry": {
                "type": "GeometryCollection",
                "geometries": [{"type": "MultiPoint", "coordinates": [[179.9, -89.9]]}],
            },
        },
    ],
}

GPX = """<?xml version="1.0"?><gpx version="1.1" creator="seed">
<wpt lat="47.54" lon="9.68"><name>Harbour</name><time>2026-11-04T09:38:00Z</time></wpt>
<rte><rtept lat="47.5" lon="9.6"/><rtept lat="47.6" lon="9.7"/></rte>
<trk><name>Walk</name><trkseg><trkpt lat="47.5" lon="9.6"><ele>400</ele></trkpt>
<trkpt lat="47.51" lon="9.61"/></trkseg></trk></gpx>
"""

CSV = (
    "name,latitude,longitude,mag,time\n"
    'Lindau,47.5458,9.6833,3.5,2026-11-04T09:38:00+01:00\n"Quoted, name",-33.9,151.2,,\n'
)

WMS = """<?xml version="1.0"?><WMS_Capabilities version="1.3.0"><Service><Title>Seed</Title>
</Service><Capability><Layer><Title>Root</Title><CRS>EPSG:3857</CRS>
<Layer queryable="1"><Name>depth</Name><Title>Depth</Title><Abstract>Sea depth</Abstract>
<Dimension name="time" default="2026-11-01">2020-01-01/2026-11-01/P1D</Dimension>
<Style><Name>default</Name><LegendURL><OnlineResource
xmlns:xlink="http://www.w3.org/1999/xlink" xlink:href="https://example.org/legend.png"/>
</LegendURL></Style><Layer><Name>depth:contours</Name><Title>Contours</Title></Layer></Layer>
</Layer></Capability></WMS_Capabilities>
"""

FEEDS = {
    "opensky.json": {
        "time": 1800000000,
        "states": [
            ["3c6444", "DLH9AB ", "Germany", 1799999990, 1799999995, 9.68, 47.54, 10972.8,
             False, 230.5, 87.2, 0.0, None, 11277.6, "1000", False, 0]
        ],
    },
    "adsb.json": {
        "now": 1800000000000,
        "ac": [
            {"hex": "3c6444", "flight": "DLH9AB", "lat": 47.54, "lon": 9.68, "alt_baro": 36000,
             "gs": 448.1, "track": 87.2, "t": "A320", "r": "D-AIZZ", "seen": 0.4, "category": "A3"},
            {"hex": "~abc", "lat": "x", "lon": 200, "alt_baro": "ground"},
        ],
    },
    "trace.json": {
        "icao": "3c6444", "timestamp": 1799990000.0,
        "trace": [[0.0, 47.5, 9.6, 1000, 250.0, 80.0, 0, 0, None],
                  [60.5, 47.6, 9.9, "ground", 10.0, 80.0, 0, 0, {"flight": "DLH9AB"}]],
    },
    "route.json": {
        "response": {"flightroute": {"callsign": "DLH9AB",
                     "origin": {"iata_code": "MUC", "latitude": 48.35, "longitude": 11.78},
                     "destination": {"iata_code": "LIS", "latitude": 38.77, "longitude": -9.13}}},
        "_airport_codes_iata": "MUC-LIS", "airport_codes": "EDDM-LPPT",
    },
    "vessels.json": {
        "type": "FeatureCollection",
        "features": [{"type": "Feature", "geometry": {"type": "Point", "coordinates": [9.5, 54.3]},
                      "properties": {"mmsi": 211000000, "name": "SEED", "cog": 123.4, "sog": 11.2,
                                     "shipType": 70, "timestampExternal": 1799999000000,
                                     "destination": "KIEL", "draught": 82}}],
        "vessels": [{"mmsi": "211000001", "lat": 54.3, "lon": 9.5, "course": 10, "speed": 3,
                     "time": 1799999000}],
    },
    "omm.json": [
        {"OBJECT_NAME": "ISS (ZARYA)", "OBJECT_ID": "1998-067A", "EPOCH": "2026-11-10T12:00:00.000000",
         "MEAN_MOTION": 15.5, "ECCENTRICITY": 0.0005, "INCLINATION": 51.64, "RA_OF_ASC_NODE": 120.1,
         "ARG_OF_PERICENTER": 80.2, "MEAN_ANOMALY": 279.9, "NORAD_CAT_ID": 25544, "BSTAR": 0.0002,
         "MEAN_MOTION_DOT": 0.0001, "MEAN_MOTION_DDOT": 0}
    ],
    "aurora.json": {
        "Observation Time": "2026-11-10T12:00:00Z", "Forecast Time": "2026-11-10T12:30:00Z",
        "coordinates": [[0, -90, 0], [0, 65, 40], [359, 90, 3]],
    },
    "timezones.json": {
        "type": "FeatureCollection",
        "features": [{"type": "Feature", "properties": {"tzid": "Europe/Berlin", "utc_offset": 60,
                      "name": "CET", "zone": 1.0, "label_lat": 51, "label_lon": 10},
                      "geometry": {"type": "Polygon",
                                   "coordinates": [[[5, 47], [15, 47], [15, 55], [5, 47]]]}}],
    },
    "marine.json": {
        "type": "FeatureCollection",
        "features": [{"type": "Feature", "properties": {"name": "Baltic Sea", "name_en": "Baltic Sea",
                      "featurecla": "sea", "scalerank": 2, "size_km": 900},
                      "geometry": {"type": "Point", "coordinates": [19, 58]}}],
    },
    "geocode.json": [
        {"lat": "47.5458", "lon": "9.6833", "display_name": "Lindau, Bayern", "name": "Lindau",
         "boundingbox": ["47.5", "47.6", "9.6", "9.7"], "type": "town", "importance": 0.5,
         "address": {"country": "Deutschland", "state": "Bayern"}}
    ],
    "photon.json": {
        "features": [{"geometry": {"coordinates": [9.6833, 47.5458]},
                      "properties": {"name": "Lindau", "country": "Germany", "state": "Bavaria",
                                     "osm_value": "town", "extent": [9.6, 47.6, 9.7, 47.5]}}],
    },
    "wayback.json": {
        "10": {"itemID": "abc", "itemTitle": "World Imagery (Wayback 2014-02-20)",
               "itemURL": "https://example.org/tile/10/{level}/{row}/{col}",
               "metadataLayerUrl": "https://example.org/m", "layerIdentifier": "WB_2014_R01"},
    },
    "manifest.json": {"version": 3, "files": {"airports": "https://example.org/airports.tsv.gz"},
                      "tiles": "https://example.org/{z}/{x}/{y}.pbf",
                      "borders": {"template": "https://example.org/borders-{band}.pmtiles"}},
    "layers.json": [
        {"name": "Depth", "kind": "wms", "url": "https://example.org/wms", "layers": "depth",
         "visible": True, "opacity": 0.8, "max_zoom": 12, "attribution": "Seed"},
        {"name": "Quakes", "kind": "features", "format": "geojson", "refresh_minutes": 5,
         "url": "https://example.org/q.geojson", "size_by": "mag", "color": "#ff8800"},
    ],
}

AIRPORTS = (
    "ident\ttype\tname\tlat\tlon\televation_ft\tiata\ticao\tcountry\tcity\n"
    "EDDM\tlarge_airport\tMunich Airport\t48.3538\t11.7861\t1487\tMUC\tEDDM\tDE\tMunich\n"
    "LPPT\tlarge_airport\tHumberto Delgado\t38.7813\t-9.1359\t374\tLIS\tLPPT\tPT\tLisbon\n"
)


def main() -> None:
    write("kml", "trip.kml", KML)
    kmz = io.BytesIO()
    with zipfile.ZipFile(kmz, "w", zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("doc.kml", KML)
        archive.writestr("files/icon.png", png(2, 2))
    write("kmz", "trip.kmz", kmz.getvalue())
    write("features", "points.geojson", b"\x00" + json.dumps(GEOJSON).encode())
    write("features", "sized.geojson", b"\x80" + json.dumps(GEOJSON).encode())
    write("features", "walk.gpx", b"\x01" + GPX.encode())
    write("features", "table.csv", b"\x02" + CSV.encode())
    write("wms_capabilities", "capabilities.xml", WMS)
    write("pmtiles", "plain.pmtiles", pmtiles(1))
    write("pmtiles", "gzip.pmtiles", pmtiles(2))
    tile = vector_tile()
    write("vector_tile", "lindau.mvt", tile_key(10, 539, 358) + tile)
    write("vector_tile", "world.mvt", tile_key(0, 0, 0) + tile)
    # A stored border tile: version 4, one two-vertex line of level 2, one named town.
    compact = bytes([4]) + varint(1) + bytes([2, 0]) + varint(2)
    compact += varint(zigzag(100)) + varint(zigzag(200)) + varint(zigzag(50)) + varint(zigzag(-20))
    compact += varint(1) + bytes([0]) + varint(zigzag(2048)) + varint(zigzag(2048)) + bytes([9])
    compact += varint(6) + b"Lindau" + bytes([1]) + varint(6) + b"Lindau" + bytes([0])
    write("compact_tile", "lindau.tile", compact)
    for pick, kind in enumerate(["imagery", "elevation", "layer"]):
        write("image_tile", f"{kind}.png", bytes([pick]) + png(16, 16, (128, 3, 40, 255)))
        write("image_tile", f"{kind}.jpg", bytes([pick]) + pil_image("JPEG"))
        write("image_tile", f"{kind}.webp", bytes([pick]) + pil_image("WEBP"))
    planet = {"version": 2, "name": "Seed", "texture": "",
              "texture_bytes": list(png(4, 2)), "sky_bytes": list(png(4, 2))}
    write("planet", "embedded.planet", b"\x00" + json.dumps(planet).encode())
    write("planet", "named.planet",
          b"\x00" + json.dumps({"version": 1, "name": "Old", "texture": "moon.png"}).encode())
    write("planet", "sky.png", b"\x01" + png(8, 4))
    for name, value in FEEDS.items():
        write("json_feeds", name, json.dumps(value))
    write("json_feeds", "route.json.gz", gzip.compress(json.dumps(FEEDS["route.json"]).encode()))
    fixtures = Path(__file__).parent.parent / "crates/globecraft-data/tests/fixtures"
    for fixture in sorted(fixtures.glob("settings-*.json")):
        write("settings", fixture.name, fixture.read_bytes())
    write("settings", "sources.json", json.dumps({"sources": FEEDS["layers.json"]}))
    documents = [json.dumps(GEOJSON).encode(), json.dumps(FEEDS["timezones.json"]).encode(),
                 json.dumps(FEEDS["marine.json"]).encode(), AIRPORTS.encode(), b"\x02\x01\x01\x05\x02"]
    for pick, document in enumerate(documents):
        write("compressed", f"{pick}.gz", bytes([pick]) + gzip.compress(document))
    write("text_inputs", "link.txt",
          "globecraft://view?lat=47.5458&lon=9.6833&alt=120&tilt=30&heading=10"
          "&layers=borders,roads&data=preset:gebco&data=https://example.org/a.geojson")
    for index, text in enumerate(["47.5458, 9.6833", "47°32'45\"N 9°41'0\"E", "S 33.9 E 151.2",
                                  "2026-11-04T09:38:00+01:00", "2026-11-04",
                                  "https://user@[::1]:8080/path?q=1", "http://192.168.1.10/",
                                  "#ff8800", "#f80a", AIRPORTS]):
        write("text_inputs", f"{index}.txt", text)
    replies = {
        "openai.json": {"id": "x", "choices": [{"message": {"role": "assistant", "content": "Hi",
                        "tool_calls": [{"id": "c1", "type": "function", "function":
                                        {"name": "run_command", "arguments": "{\"id\":\"camera.flyTo\"}"}}]},
                        "finish_reason": "tool_calls"}],
                        "usage": {"prompt_tokens": 10, "completion_tokens": 5,
                                  "prompt_tokens_details": {"cached_tokens": 2}}},
        "anthropic.json": {"type": "message", "content": [{"type": "text", "text": "Hi"},
                           {"type": "tool_use", "id": "t1", "name": "run_command",
                            "input": {"id": "camera.flyTo"}}], "stop_reason": "tool_use",
                           "usage": {"input_tokens": 10, "output_tokens": 5,
                                     "cache_read_input_tokens": 2}},
        "gemini.json": {"candidates": [{"content": {"parts": [{"text": "Hi"},
                        {"functionCall": {"name": "run_command", "args": {"id": "camera.flyTo"}}}]},
                        "finishReason": "STOP"}],
                        "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 5}},
        "models.json": {"data": [{"id": "model-a"}], "models": [{"name": "models/model-b"}]},
        "error.json": {"error": {"message": "rate limited", "type": "rate_limit_error", "code": 429}},
    }
    pick = 0
    for name, value in replies.items():
        for api in range(3):
            write("assistant_reply", f"{api}-{name}", bytes([api + 3 * pick]) + json.dumps(value).encode())
        pick = (pick + 1) % 5
    stream = (
        'data: {"choices":[{"delta":{"content":"Hi"}}]}\n\n'
        'data: {"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5}}\n\ndata: [DONE]\n\n'
    )
    write("assistant_reply", "0-stream.txt", b"\x00" + stream.encode())
    event = (
        'event: message_start\ndata: {"type":"message_start","message":{"usage":{"input_tokens":3}}}\n\n'
        'event: content_block_delta\ndata: {"type":"content_block_delta","index":0,'
        '"delta":{"type":"text_delta","text":"Hi"}}\n\n'
        'event: message_delta\ndata: {"type":"message_delta","delta":{"stop_reason":"end_turn"},'
        '"usage":{"output_tokens":5}}\n\n'
    )
    write("assistant_reply", "1-stream.txt", b"\x01" + event.encode())
    # MBTiles rows are structured input (see the target); libFuzzer builds them from bytes.
    write("mbtiles_rows", "rows.bin", bytes(range(64)))


if __name__ == "__main__":
    main()

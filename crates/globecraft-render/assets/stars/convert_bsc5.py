"""Converts the Yale Bright Star Catalogue (BSC5) into the app's star table.

Source: http://tdc-www.harvard.edu/catalogs/BSC5 (Harvard-Smithsonian Center for
Astrophysics, Telescope Data Center; 9,110 stars, J2000 positions, PC byte order),
downloaded 2026-11-18. Run: python3 convert_bsc5.py BSC5 bsc5.bin

Each source entry is 32 bytes: catalogue number (f32), RA and Dec at J2000 (f64,
radians), spectral type (2 chars), V magnitude × 100 (i16), proper motion in RA and
Dec (f32, radians a year). The table keeps, per star with a magnitude, 8 bytes
little-endian: RA as u16 (0..65536 over a turn), Dec as i16 (±32767 over ±90°),
V magnitude × 100 as i16, and the spectral class letter as one byte (and a zero).
Positions are moved by their proper motion to 2026.0 (at most a few arcseconds here).
"""
import math
import struct
import sys

EPOCH_YEARS = 26.0  # J2000 to 2026.0

source, target = sys.argv[1], sys.argv[2]
data = open(source, "rb").read()
star0, star1, starn, stnum, mprop, nmag, nbent = struct.unpack("<7i", data[:28])
assert starn == -9110 and nbent == 32, (starn, nbent)
out = bytearray()
kept = 0
for i in range(-starn):
    entry = data[28 + i * 32 : 28 + (i + 1) * 32]
    number, ra, dec, spectral, mag, pm_ra, pm_dec = struct.unpack("<fdd2shff", entry)
    if mag == 0 and ra == 0 and dec == 0:
        continue  # removed entries (novae, galaxies) have no position
    ra = (ra + pm_ra * EPOCH_YEARS) % (2 * math.pi)
    dec = max(-math.pi / 2, min(math.pi / 2, dec + pm_dec * EPOCH_YEARS))
    letter = spectral[:1] if spectral[:1].isalpha() else b"?"
    out += struct.pack(
        "<HhhBB",
        round(ra / (2 * math.pi) * 65536) % 65536,
        round(dec / (math.pi / 2) * 32767),
        mag,
        letter[0],
        0,
    )
    kept += 1
open(target, "wb").write(out)
print(f"{kept} stars, {len(out)} bytes")

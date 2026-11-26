# Star table

`bsc5.bin` holds the 9,096 stars of the Yale Bright Star Catalogue, 5th revised
edition (Hoffleit & Warren), that have a position: every star to about V = 6.5, the
naked-eye sky. It was made from Harvard's binary copy
(<http://tdc-www.harvard.edu/catalogs/BSC5>, downloaded 2026-11-18) by
`convert_bsc5.py`, which documents both formats. The catalogue is distributed freely
by its compilers and NASA HEASARC / CDS.

Positions are J2000 (ICRS), moved by proper motion to 2026.0. `globecraft_core`'s
`CelestialFrame` turns them into Earth-fixed directions for a moment in time, as it
does the Milky Way.

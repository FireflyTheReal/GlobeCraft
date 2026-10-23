"""Per-pair border audit metrics from check.json (see the GlobeCraft harnesses)."""
import json, math, sys, collections
import numpy as np

KM_LAT = 110.57
def km_lon(lat): return 111.32 * max(math.cos(math.radians(lat)), 0.05)

class Segments:
    """Grid of line segments in degrees; exact distances in a local km frame."""
    def __init__(self, lines, cell=0.25):
        self.cell = cell; self.grid = collections.defaultdict(list); a=[]; b=[]; run=[]
        for r, line in enumerate(lines):
            for p, q in zip(line, line[1:]):
                if p == q: continue
                i = len(a); a.append(p); b.append(q); run.append(r)
                y0, y1 = sorted((p[0], q[0])); x0, x1 = sorted((p[1], q[1]))
                for y in range(math.floor(y0 / cell), math.floor(y1 / cell) + 1):
                    for x in range(math.floor(x0 / cell), math.floor(x1 / cell) + 1):
                        self.grid[(y, x)].append(i)
        self.a = np.array(a, float).reshape(-1, 2); self.b = np.array(b, float).reshape(-1, 2)
        self.run = np.array(run, int)
        self.length_km = sum(math.hypot((q[1]-p[1])*km_lon(p[0]), (q[0]-p[0])*KM_LAT) for p, q in zip(a, b))
    def nearest_km(self, lat, lon, reach_km, exclude_run=None):
        """Distance to the nearest segment, or inf beyond reach_km."""
        r_lat = reach_km / KM_LAT; r_lon = reach_km / km_lon(lat)
        idx = set()
        for y in range(math.floor((lat - r_lat) / self.cell), math.floor((lat + r_lat) / self.cell) + 1):
            for x in range(math.floor((lon - r_lon) / self.cell), math.floor((lon + r_lon) / self.cell) + 1):
                idx.update(self.grid.get((y, x), ()))
        if not idx: return math.inf
        idx = np.fromiter(idx, int)
        if exclude_run is not None:
            idx = idx[self.run[idx] != exclude_run]
            if idx.size == 0: return math.inf
        kx, ky = km_lon(lat), KM_LAT
        ax = (self.a[idx, 1] - lon) * kx; ay = (self.a[idx, 0] - lat) * ky
        bx = (self.b[idx, 1] - lon) * kx; by = (self.b[idx, 0] - lat) * ky
        dx, dy = bx - ax, by - ay; L = dx * dx + dy * dy
        t = np.clip(-(ax * dx + ay * dy) / np.where(L > 0, L, 1), 0, 1)
        d = np.hypot(ax + t * dx, ay + t * dy).min()
        return d if d <= reach_km else math.inf

def inside(area, lat, lon):
    clat, clon, r = area
    lr = r / max(math.cos(math.radians(clat)), 0.2)
    return abs(lat - clat) <= r and abs(lon - clon) <= lr

def sample(line, step_km=1.0):
    for p, q in zip(line, line[1:]):
        L = math.hypot((q[1]-p[1])*km_lon(p[0]), (q[0]-p[0])*KM_LAT)
        n = max(1, int(L / step_km))
        for i in range(n):
            t = (i + 0.5) / n
            yield (p[0] + t*(q[0]-p[0]), p[1] + t*(q[1]-p[1])), L / n

def pair_lines(ne_file, a, b):
    out = []
    for f in json.load(open(ne_file))['features']:
        pr = f['properties']
        if {pr.get('ADM0_A3_L'), pr.get('ADM0_A3_R')} != {a, b}: continue
        g = f['geometry']; ls = g['coordinates'] if g['type'] == 'MultiLineString' else [g['coordinates']]
        out += [[(y, x) for x, y in l] for l in ls]
    return out

def audit(check, ne_file, a, b, area=None):
    d = json.load(open(check))
    muni = Segments(d['country']); kept = Segments(d['ne_country'])
    outline = Segments(d['interior'])
    ne_all_lines = [[(y, x) for x, y in l] for f in json.load(open(ne_file))['features']
                    for l in ((f['geometry']['coordinates'],) if f['geometry']['type']=='LineString' else f['geometry']['coordinates'])]
    every_ne = Segments(ne_all_lines)
    ne_ends = Segments([[l[0], l[0]] for l in ne_all_lines] + [[l[-1], l[-1]] for l in ne_all_lines])
    km = collections.Counter(); spots = collections.defaultdict(list)
    for line in pair_lines(ne_file, a, b):
        for (lat, lon), w in sample(line):
            if area and not inside(area, lat, lon): continue
            is_kept = kept.nearest_km(lat, lon, 0.2) < math.inf
            m = muni.nearest_km(lat, lon, 15.0)
            if not is_kept:
                cls = 'replaced' if m < math.inf else 'lost'
            elif m < math.inf:
                cls = 'double'
            else:
                cls = 'coarse_data' if outline.nearest_km(lat, lon, 8.0) < math.inf else 'coarse_nodata'
            km[cls] += w; spots[cls].append((round(lat, 3), round(lon, 3)))
    # Visible breaks: an end of a drawn line (municipal country or kept coarse) that
    # meets no other drawn line, away from a Natural Earth line end (coast, tripoint)
    # and from the edge of the area.
    drawn_lines = [l for l in d['country'] if len(l) > 1] + [l for l in d['ne_country'] if len(l) > 1]
    drawn = Segments(drawn_lines)
    pair_seg = Segments(pair_lines(ne_file, a, b))
    breaks = 0
    for r, line in enumerate(drawn_lines):
        for p in (line[0], line[-1]):
            lat, lon = p
            if area and not inside((area[0], area[1], area[2] - 0.3), lat, lon): continue
            if pair_seg.nearest_km(lat, lon, 15.0) == math.inf: continue
            if ne_ends.nearest_km(lat, lon, 5.0) < math.inf: continue
            if drawn.nearest_km(lat, lon, 0.3, exclude_run=r) < math.inf: continue
            breaks += 1; spots['break'].append((round(lat, 3), round(lon, 3)))
    km['breaks'] = breaks
    stray = 0.0
    for line in d['country']:
        for (lat, lon), w in sample(line, 0.5):
            if every_ne.nearest_km(lat, lon, 10.0) == math.inf: stray += w
    km['stray_muni'] = stray; km['muni_total'] = muni.length_km
    return {k: round(v, 1) for k, v in km.items()}, {k: v[::max(1, len(v)//40)] for k, v in spots.items()}

if __name__ == '__main__':
    check, ne_file, a, b = sys.argv[1:5]
    area = tuple(map(float, sys.argv[5].split(','))) if len(sys.argv) > 5 else None
    km, spots = audit(check, ne_file, a, b, area)
    print(json.dumps({'a': a, 'b': b, 'km': km, 'spots': spots}))

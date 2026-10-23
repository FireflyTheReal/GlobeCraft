"""Lists the land borders the audit covers: Natural Earth 1:10m country pairs where
both countries load municipal borders from geoBoundaries (national sources excluded).
Usage: make_pairs.py <data dir>  ->  <data dir>/audit/pairs.json"""
import collections, json, math, os, re, sys

ROOT = os.path.join(os.path.dirname(__file__), '..', '..')
ADM = open(os.path.join(ROOT, 'crates/globecraft-data/src/boundaries/adm.rs')).read()
ALIAS = {"PSX": "PSE", "SDS": "SSD", "KOS": "XKX", "SAH": "ESH"}
NATIONAL = {'DEU', 'FRA', 'ESP', 'USA', 'CAN', 'BRA'}

def levels():
    out = {}
    for level in ('ADM1', 'ADM2', 'ADM3', 'ADM4'):
        codes = re.search(level + r'_COUNTRIES: &str = "(.*?)";', ADM, re.S).group(1)
        out.update({code: level for code in codes.replace('\\', '').split()})
    return out

def main(data):
    level = levels()
    pairs = collections.defaultdict(lambda: [0.0, [180, -180, 90, -90]])
    lines = json.load(open(os.path.join(data, 'ne_10m_admin_0_boundary_lines_land.geojson')))
    for feature in lines['features']:
        p = feature['properties']; a, b = p.get('ADM0_A3_L'), p.get('ADM0_A3_R')
        if not a or not b or a == b: continue
        key = tuple(sorted((a, b))); g = feature['geometry']
        for line in (g['coordinates'] if g['type'] == 'MultiLineString' else [g['coordinates']]):
            for (x1, y1), (x2, y2) in zip(line, line[1:]):
                pairs[key][0] += math.hypot((x2 - x1) * math.cos(math.radians(y1)) * 111.32, (y2 - y1) * 110.57)
            for x, y in line:
                bb = pairs[key][1]
                bb[0], bb[1], bb[2], bb[3] = min(bb[0], x), max(bb[1], x), min(bb[2], y), max(bb[3], y)
    out = [dict(a=a, b=b, km=round(km), bbox=bbox)
           for (a, b), (km, bbox) in sorted(pairs.items(), key=lambda item: -item[1][0])
           if not ({a, b} & NATIONAL) and ALIAS.get(a, a) in level and ALIAS.get(b, b) in level]
    os.makedirs(os.path.join(data, 'audit'), exist_ok=True)
    json.dump(out, open(os.path.join(data, 'audit', 'pairs.json'), 'w'))
    print(len(out), 'pairs')

if __name__ == '__main__':
    main(sys.argv[1])

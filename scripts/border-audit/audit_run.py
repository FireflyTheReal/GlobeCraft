"""Runs the app's municipal border code on every auditable land border."""
import json, math, os, subprocess, sys, time
sys.path.insert(0, os.path.dirname(__file__))
from audit_metrics import audit
G, data_bin, render_bin, out = sys.argv[1:5]
only = set(sys.argv[5].split(',')) if len(sys.argv) > 5 else None
pairs = json.load(open(G + '/audit/pairs.json'))
done = set()
if os.path.exists(out):
    for l in open(out):
        r = json.loads(l); done.add((r['a'], r['b']))
ne = G + '/ne_10m_admin_0_boundary_lines_land.geojson'
for p in pairs:
    a, b = p['a'], p['b']
    if (a, b) in done or (only and f'{a}-{b}' not in only): continue
    x0, x1, y0, y1 = p['bbox']
    lat, lon = (y0 + y1) / 2, (x0 + x1) / 2
    r = max((y1 - y0) / 2, (x1 - x0) / 2 * max(math.cos(math.radians(lat)), 0.2)) + 0.2
    env = dict(os.environ, GLOBECRAFT_MUNICIPAL_CHECK=G, GLOBECRAFT_MUNICIPAL_COUNTRIES=f'{a},{b}',
               GLOBECRAFT_MUNICIPAL_AREA=f'{lat},{lon},{r}', GLOBECRAFT_HANDOVER_CHECK=G + '/check.json')
    t = time.time(); rec = dict(a=a, b=b, ne_km=p['km'], area=[lat, lon, r])
    try:
        d = subprocess.run([data_bin, 'real_countries_classify', '--ignored', '--nocapture'], env=env,
                           capture_output=True, text=True, timeout=1800)
        if 'test result: ok' not in d.stdout:
            rec['error'] = (d.stdout + d.stderr)[-600:]
        else:
            subprocess.run([render_bin, 'real_cut_of_natural', '--ignored', '--nocapture'], env=env,
                           capture_output=True, text=True, timeout=1800, check=True)
            rec['km'], rec['spots'] = audit(G + '/check.json', ne, a, b, (lat, lon, r))
    except Exception as e:
        rec['error'] = repr(e)[-600:]
    rec['seconds'] = round(time.time() - t, 1)
    with open(out, 'a') as f: f.write(json.dumps(rec) + '\n')
    print(a, b, rec.get('km', rec.get('error', ''))[:0] if False else '', rec.get('seconds'), 'ERR' if 'error' in rec else '', flush=True)
print('done')

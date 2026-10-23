import json, sys, math
import matplotlib; matplotlib.use('Agg'); import matplotlib.pyplot as plt
sys.path.insert(0, __import__('os').path.dirname(__file__))
from audit_metrics import audit
check, ne, a, b, out, view = sys.argv[1:7]
lat0, lon0, r = map(float, view.split(','))
d = json.load(open(check)); k = math.cos(math.radians(lat0))
fig, ax = plt.subplots(figsize=(13, 10)); ax.set_facecolor('#2a3328')
def draw(lines, col, lw, ls='-', z=1):
    for l in lines:
        if any(abs(p[0]-lat0) < r*1.3 and abs(p[1]-lon0) < r/k*1.3 for p in l):
            ax.plot([p[1] for p in l], [p[0] for p in l], color=col, lw=lw, ls=ls, zorder=z)
draw(d['interior'], '#8a9a8a', 0.5)
draw(d['ne_country_all'], '#ffe000', 1.2, '--', 2)
draw(d['ne_country'], '#ffffff', 2.2, '-', 3)
draw(d['country'], '#ff3a6a', 2.0, '-', 4)
km, spots = audit(check, ne, a, b, (lat0, lon0, r))
colors = {'lost': 'red', 'double': 'cyan', 'coarse_data': 'orange', 'break': 'magenta'}
for cls, col in colors.items():
    pts = spots.get(cls, [])
    ax.scatter([p[1] for p in pts], [p[0] for p in pts], s=18, c=col, zorder=5, label=cls)
ax.set_xlim(lon0 - r/k, lon0 + r/k); ax.set_ylim(lat0 - r, lat0 + r); ax.set_aspect(1/k)
ax.legend(loc='lower left'); ax.set_title(f'{a}-{b} {km} | yellow dashed NE all, white NE kept, pink municipal country, grey municipal')
plt.savefig(out, dpi=90, bbox_inches='tight')

import json, sys, collections
rows = [json.loads(l) for l in open(sys.argv[1])]
errs = [r for r in rows if 'error' in r]
ok = [r for r in rows if 'km' in r]
tot = collections.Counter()
for r in ok: tot.update(r['km'])
ne = sum(tot[k] for k in ('replaced', 'gap', 'double', 'coarse_data', 'coarse_nodata'))
print(f'{len(ok)} pairs audited, {len(errs)} errors, {ne:,.0f} km of border sampled')
for k in ('replaced', 'gap', 'double', 'coarse_data', 'coarse_nodata'):
    print(f'  {k:<14} {tot[k]:>9,.0f} km  {100*tot[k]/max(ne,1):5.1f} %')
print(f'  stray municipal {tot["stray_muni"]:,.0f} km of {tot["muni_total"]:,.0f} km drawn')
def worst(key, n=12):
    rs = sorted(ok, key=lambda r: -r['km'].get(key, 0))[:n]
    print(f'worst by {key}: ' + ', '.join(f"{r['a']}-{r['b']} {r['km'].get(key,0):.0f}" for r in rs))
for k in ('gap', 'double', 'coarse_data', 'stray_muni'): worst(k)
if errs: print('errors:', [(r['a'], r['b'], r['error'][-160:]) for r in errs[:5]])

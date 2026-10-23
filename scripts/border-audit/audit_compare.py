import json, sys, collections
def load(p):
    return {(r['a'], r['b']): r for r in map(json.loads, open(p)) if 'km' in r}
base, new = load(sys.argv[1]), load(sys.argv[2])
keys = sorted(set(base) & set(new))
classes = ('replaced', 'lost', 'double', 'coarse_data', 'coarse_nodata')
def totals(rows):
    t = collections.Counter()
    for k in keys: t.update(rows[k]['km'])
    return t
tb, tn = totals(base), totals(new)
ne = sum(tb[c] for c in classes)
print(f'{len(keys)} pairs, {ne:,.0f} km of Natural Earth border')
print(f'{"":<14}{"before":>12}{"after":>12}')
for c in classes:
    print(f'{c:<14}{tb[c]:>10,.0f} km{tn[c]:>10,.0f} km   ({100*tb[c]/ne:4.1f}% -> {100*tn[c]/ne:4.1f}%)')
print(f'{"breaks":<14}{tb["breaks"]:>13,.0f}{tn["breaks"]:>12,.0f}')
print(f'{"stray km":<14}{tb["stray_muni"]:>12,.0f}{tn["stray_muni"]:>12,.0f}')
def delta(c):
    return sorted(keys, key=lambda k: new[k]['km'].get(c, 0) - base[k]['km'].get(c, 0))
for c in ('double', 'breaks', 'lost'):
    d = delta(c)
    print(f'most improved {c}: ' + ', '.join(f"{a}-{b} {base[(a,b)]['km'].get(c,0):.0f}->{new[(a,b)]['km'].get(c,0):.0f}" for a, b in d[:6]))
    print(f'most worsened {c}: ' + ', '.join(f"{a}-{b} {base[(a,b)]['km'].get(c,0):.0f}->{new[(a,b)]['km'].get(c,0):.0f}" for a, b in d[-6:][::-1]))

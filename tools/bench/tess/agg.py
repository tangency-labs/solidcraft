#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Tables of the mesher bake-off: agg.py RUNS LIST OUT.md

RUNS holds one folder per mesher (sc-old, core-old, combined), each the OUT of run.sh; LIST is the
bodies' names. Writes the tables (not the prose) of results.md and prints the regressions of the
last mesher against the best of the other two."""
import json, os, sys, statistics as st
from collections import defaultdict

RUNS, LIST, OUTMD = sys.argv[1], sys.argv[2], sys.argv[3]
LABELS = ['sc-old', 'core-old', 'combined']
NAMES = {'sc-old': "SolidCraft's old mesher", 'core-old': 'solidcore-tess (old)', 'combined': 'combined mesher'}
MODES = ['display', 'export']
bodies = [l.strip() for l in open(LIST) if l.strip()]
bset = set(bodies)

R = {}      # (label, mode, body) -> result row
PROC = {}   # (label, mode, body) -> rc
KINDS, INFO = {}, {}
for lab in LABELS:
    for mode in MODES:
        d = os.path.join(RUNS, lab, mode)
        if not os.path.isdir(d):
            continue
        for fn in os.listdir(d):
            if not fn.endswith('.jsonl'):
                continue
            b = fn[:-6]
            if b not in bset:
                continue
            for l in open(os.path.join(d, fn)):
                l = l.strip()
                if not l.startswith('{'):
                    continue
                x = json.loads(l)
                k = x.get('kind')
                if k == 'result':
                    R[(lab, mode, b)] = x
                elif k == 'proc':
                    PROC[(lab, mode, b)] = x['rc']
                elif k == 'kinds':
                    KINDS[b] = x['kinds']
                elif k == 'body':
                    INFO[(b, mode)] = x['info']
                elif 'error' in x and 'mesher' not in x:
                    pass
for (lab, mode, b), x in R.items():
    x['chord'] = INFO.get((b, mode), {}).get('chord', float('nan'))

out = []
def w(s=""): out.append(s)
def q(v, p):
    v = sorted(v)
    return v[int((len(v) - 1) * p)] if v else float('nan')
def fmt(x, n=3):
    return f'{x:.{n}f}' if isinstance(x, float) else str(x)
def rows(lab, mode, ok=True):
    return [R[(lab, mode, b)] for b in bodies if (lab, mode, b) in R and (not ok or R[(lab, mode, b)].get('ok'))]

def metrics(lab, mode):
    allr = [R[(lab, mode, b)] for b in bodies if (lab, mode, b) in R]
    nproc = sum(1 for b in bodies if (lab, mode, b) not in R)
    ok = [r for r in allr if r.get('ok')]
    dv = [r for r in ok if 'dev_max' in r]
    ratio = [r['dev_max'] / r['chord'] for r in dv]
    closed = [r for r in ok if r['brep_boundary_edges'] == 0]
    t = [r['ms'] for r in ok]
    m = {
        'bodies run': len(bodies), 'meshed ok': len(ok), 'failed': len(bodies) - len(ok),
        'triangles': sum(r['tris'] for r in ok),
        'bodies with a face missing': sum(1 for r in ok if r['faces_without_tris'] > 0),
        'max dev / chord (median body)': st.median(ratio) if ratio else float('nan'),
        '(p95)': q(ratio, .95),
        'bodies over chord': sum(1 for x in ratio if x > 1.0001),
        'sample points over chord': sum(r['dev_over_chord'] for r in dv),
        'closed bodies with open edges': sum(1 for r in closed if r['open_edges'] > 0),
        '... of which true gaps': sum(1 for r in closed if r['open_unsealed'] > 0),
        'nonmanifold-edge bodies': sum(1 for r in ok if r['nonmanifold_edges'] > 0),
        'tris with min angle <1 deg': sum(r['tris_lt1deg'] for r in ok),
        '<5 deg': sum(r['tris_lt5deg'] for r in ok),
        'worst aspect (p99 of bodies, median)': st.median(r['p99_aspect'] for r in ok) if ok else float('nan'),
        'time median ms': st.median(t) if t else float('nan'), 'p95 ms': q(t, .95), 'slowest ms': max(t) if t else float('nan'),
    }
    m['_dev_samples'] = sum(r['dev_samples'] for r in dv)
    m['_closed'] = len(closed)
    return m

HIGHER_BETTER = {'meshed ok'}
NEUTRAL = {'bodies run', 'triangles'}
def verdict(key, c, a, b):
    """combined c against the better of the two old meshers a, b."""
    if key in NEUTRAL:
        best = min(a, b)
        return f'{(c - best) / max(best, 1) * 100:+.2f}% vs fewest' if key == 'triangles' else '-'
    best = max(a, b) if key in HIGHER_BETTER else min(a, b)
    d = c - best
    if abs(d) <= 1e-9 * max(1, abs(best)) or (isinstance(c, float) and abs(d) < 0.005 * max(abs(best), 1e-9)):
        return 'equal'
    worse = d < 0 if key in HIGHER_BETTER else d > 0
    if isinstance(c, float):
        return f'{(d / max(abs(best), 1e-12)) * 100:+.1f}% ' + ('worse' if worse else 'better')
    return f'{d:+d} ' + ('worse' if worse else 'better')

w('## 1. Per-mesher totals')
w()
KEYS = ['bodies run', 'meshed ok', 'failed', 'triangles', 'bodies with a face missing', 'max dev / chord (median body)', '(p95)',
        'bodies over chord', 'sample points over chord', 'closed bodies with open edges', '... of which true gaps',
        'nonmanifold-edge bodies', 'tris with min angle <1 deg', '<5 deg', 'worst aspect (p99 of bodies, median)',
        'time median ms', 'p95 ms', 'slowest ms']
M = {}
for mode in MODES:
    w(f'### {mode} setting ({"chord = diagonal/1000 and facet angle 20 deg" if mode == "display" else "chord = diagonal/5000, chord only (no angle)"})')
    w()
    w('| mesher | ' + ' | '.join(KEYS) + ' |')
    w('|---|' + '---|' * len(KEYS))
    for lab in LABELS:
        m = M[(lab, mode)] = metrics(lab, mode)
        cells = []
        for k in KEYS:
            v = m[k]
            if k == 'triangles' or k.startswith('tris with') or k == '<5 deg' or k == 'sample points over chord':
                v = f'{v:,}'
                if k == 'sample points over chord':
                    v += f' of {m["_dev_samples"]:,}'
            elif k == 'closed bodies with open edges':
                v = f'{v} of {m["_closed"]}'
            elif k in ('time median ms', 'worst aspect (p99 of bodies, median)'):
                v = fmt(v, 2)
            elif k in ('p95 ms',):
                v = fmt(v, 1)
            elif k in ('slowest ms',):
                v = fmt(v, 0)
            elif isinstance(v, float):
                v = fmt(v)
            cells.append(str(v))
        w(f'| {lab} | ' + ' | '.join(cells) + ' |')
    cm = [verdict(k, M[('combined', mode)][k], M[('sc-old', mode)][k], M[('core-old', mode)][k]) for k in KEYS]
    w('| **combined vs best of the two** | ' + ' | '.join(cm) + ' |')
    w()

# ---- by kind
def area(b):
    if b.startswith('mesh-bakeoff-stepcases__'): return 'step'
    if b.startswith('mesh-bakeoff-excases__'): return 'examples'
    if b.startswith('prim-'): return 'primitives'
    return b.split('__')[0]
def kindclass(b):
    k = set(KINDS.get(b, []))
    if not k: return '?'
    if k <= {'Plane', 'PlaneThrough'}: return 'planar only'
    if k & {'Spline', 'RationalSpline', 'PlacedRevolution', 'BallPatch'}: return 'has spline / revolution / ball patch faces'
    return 'analytic curved (cylinder, cone, sphere, torus)'
def cell(mode, lab, sel):
    ok = [R[(lab, mode, b)] for b in bodies if sel(b) and (lab, mode, b) in R and R[(lab, mode, b)].get('ok')]
    if not ok: return None
    dv = [r for r in ok if 'dev_max' in r]
    ratio = [r['dev_max'] / r['chord'] for r in dv]
    closed = [r for r in ok if r['brep_boundary_edges'] == 0]
    gaps = [r for r in closed if r['open_unsealed'] > 0]
    tj = [r for r in closed if r['open_edges'] > 0 and r['open_unsealed'] == 0]
    return dict(n=len(ok), tris=sum(r['tris'] for r in ok), over=sum(1 for x in ratio if x > 1.0001), gaps=len(gaps), tj=len(tj),
                miss=sum(1 for r in ok if r['faces_without_tris'] > 0), tmed=st.median(r['ms'] for r in ok), tsum=sum(r['ms'] for r in ok),
                failed=sum(1 for b in bodies if sel(b) and not ((lab, mode, b) in R and R[(lab, mode, b)].get('ok'))))
groups = [('primitives', lambda b: area(b) == 'primitives'),
  ('planar-only bodies', lambda b: kindclass(b) == 'planar only' and area(b) != 'primitives'),
  ('analytic curved (cyl/cone/sphere/torus + planes)', lambda b: kindclass(b).startswith('analytic') and area(b) != 'primitives'),
  ('spline / revolution / ball-patch faces (trimmed NURBS, lofts, sweeps)', lambda b: kindclass(b).startswith('has spline')),
  ('fillets area', lambda b: area(b) == 'fillets'), ('sweeps area (coils, lofts, sweeps)', lambda b: area(b) == 'sweeps'),
  ('threads area', lambda b: area(b) == 'threads'), ('sheet-metal area', lambda b: area(b) == 'sheet-metal'),
  ('booleans area', lambda b: area(b) == 'booleans'), ('crashes area (stress)', lambda b: area(b) == 'crashes'),
  ('surfaces area', lambda b: area(b) == 'surfaces'), ('shell/offset area', lambda b: area(b) == 'shell'),
  ('import (kernel-cases) area', lambda b: area(b) == 'import'),
  ('STEP files (conformance, regressions, NIST, models-corpus)', lambda b: area(b) == 'step'),
  ('examples/*.json designs', lambda b: area(b) == 'examples'),
  ('everything else in kernel-cases', lambda b: area(b) in ('assemblies', 'construction', 'continuity', 'frames', 'history', 'inspect', 'ladder', 'plastic', 'solids', 'tessellation'))]
def cmp_cell(c, a, b):
    worse = []
    for k, lbl in (('failed', 'failed'), ('over', 'over-chord'), ('gaps', 'gap'), ('miss', 'face-missing')):
        if c[k] > min(a[k], b[k]): worse.append(f'{lbl} +{c[k] - min(a[k], b[k])}')
    if c['tj'] > min(a['tj'], b['tj']): worse.append(f'T-jn +{c["tj"] - min(a["tj"], b["tj"])}')
    best = min(a['tmed'], b['tmed'])
    if c['tmed'] > best * 1.1 + 0.005: worse.append(f'median ms {c["tmed"] / best:.2f}x')
    return 'worse: ' + ', '.join(worse) if worse else 'best or equal'
w('## 2. By kind of body')
w()
w('Per cell: tris / bodies over the chord / closed bodies with a true gap / closed bodies with only T-junctions / bodies with a face missing / median ms. The last column compares the combined mesher with the better of the two old ones on counts (and median time, 10 percent slack).')
w()
for mode in ('export', 'display'):
    w(f'### {mode}')
    w()
    w('| body group | bodies | ' + ' | '.join(f'{l}: tris / over-chord / gap / T-jn / face-missing / med ms' for l in LABELS) + ' | combined vs best of the two |')
    w('|---|---|' + '---|' * (len(LABELS) + 1))
    for name, sel in groups:
        cells = [cell(mode, l, sel) for l in LABELS]
        if not all(cells):
            continue
        w(f'| {name} | {cells[0]["n"]} | ' + ' | '.join(f'{c["tris"]:,} / {c["over"]} / {c["gaps"]} / {c["tj"]} / {c["miss"]} / {c["tmed"]:.2f}' for c in cells) + f' | {cmp_cell(cells[2], cells[0], cells[1])} |')
    w()

# ---- failures and regressions
w('## 3. Where one mesher fails or differs from another')
w()
def get(lab, mode, b): return R.get((lab, mode, b))
w('Meshing failures (a mesher returned an error, panicked or was killed), by body:')
w()
w('| body | setting | ' + ' | '.join(LABELS) + ' |')
w('|---|---|' + '---|' * len(LABELS))
nf = 0
for b in bodies:
    for mode in MODES:
        st_ = []
        bad = False
        for l in LABELS:
            r = get(l, mode, b)
            if r is None: st_.append(f'no result (rc {PROC.get((l, mode, b), "?")})'); bad = True
            elif not r.get('ok'): st_.append(str(r.get('error'))[:90]); bad = True
            else: st_.append('ok')
        if bad:
            nf += 1; w(f"| {b.split('__', 1)[-1]} | {mode} | " + ' | '.join(st_) + ' |')
if not nf: w('| none | | | | |')
w()
w('Bodies where a mesher leaves a face with no triangles:')
w()
for l in LABELS:
    mm = [(b.split('__', 1)[-1], mo, get(l, mo, b)['faces_without_tris'], get(l, mo, b)['faces']) for b in bodies for mo in MODES if get(l, mo, b) and get(l, mo, b).get('ok') and get(l, mo, b)['faces_without_tris'] > 0]
    w(f'- **{l}**: {len(mm)} body-settings; ' + '; '.join(f'{b} {mo} ({a}/{f} faces empty)' for b, mo, a, f in mm[:8]))
w()

# regressions of combined vs the best old
def dev_ratio(r): return r['dev_max'] / r['chord'] if r and r.get('ok') and 'dev_max' in r else None
CATS = [
    ('failed to mesh', lambda r: 0 if r and r.get('ok') else 1, 0),
    ('faces left empty', lambda r: r['faces_without_tris'] if r and r.get('ok') else 10**9, 0),
    ('closed body, true gap in the mesh', lambda r: (r['open_unsealed'] if r['brep_boundary_edges'] == 0 else 0) if r and r.get('ok') else 10**9, 0),
    ('closed body, open edges (gap or T-junction)', lambda r: (r['open_edges'] if r['brep_boundary_edges'] == 0 else 0) if r and r.get('ok') else 10**9, 0),
    ('non-manifold edges', lambda r: r['nonmanifold_edges'] if r and r.get('ok') else 10**9, 0),
    ('flipped edges', lambda r: r['flipped_edges'] if r and r.get('ok') else 10**9, 0),
    ('degenerate triangles', lambda r: r['degenerate_tris'] if r and r.get('ok') else 10**9, 0),
    ('max deviation over the chord (more than 5 percent worse)', lambda r: max(1.0, dev_ratio(r) or 0) if r and r.get('ok') else 10**9, 0.05),
    ('triangles under 1 degree (more than 10 percent worse, and 10 more)', lambda r: r['tris_lt1deg'] if r and r.get('ok') else 10**9, 'sliver'),
    ('triangles (more than 10 percent over the smaller old count, and 100 more)', lambda r: r['tris'] if r and r.get('ok') else 10**9, 'tris'),
]
REG = {}
for cname, f, slack in CATS:
    for mode in MODES:
        for b in bodies:
            a, o, c = get('sc-old', mode, b), get('core-old', mode, b), get('combined', mode, b)
            if c is None: continue
            fa, fo, fc = f(a), f(o), f(c)
            best = min(fa, fo)
            if best >= 10**9 and fc >= 10**9: continue
            if slack == 0: bad = fc > best
            elif slack == 'sliver': bad = fc > best * 1.1 + 10
            elif slack == 'tris': bad = fc > best * 1.1 + 100
            else: bad = fc > best * (1 + slack)
            if bad: REG.setdefault((cname, mode), []).append((b, best, fc))
# time
for mode in MODES:
    for b in bodies:
        a, o, c = get('sc-old', mode, b), get('core-old', mode, b), get('combined', mode, b)
        if a and o and c and a.get('ok') and o.get('ok') and c.get('ok'):
            best = min(a['ms'], o['ms'])
            if c['ms'] > best * 1.5 and c['ms'] - best > 20: REG.setdefault(('time: over 1.5x the faster old mesher and over 20 ms more', mode), []).append((b, best, c['ms']))
w('### Where the combined mesher is worse than the better of the two old meshers')
w()
w('Per body and setting, combined against the better old value of the same measure (so a body counts where either old mesher did better).')
w()
w('| measure | display: bodies | export: bodies | worst examples (body, setting, best old -> combined) |')
w('|---|---|---|---|')
for cname in [c[0] for c in CATS] + ['time: over 1.5x the faster old mesher and over 20 ms more']:
    d, e = REG.get((cname, 'display'), []), REG.get((cname, 'export'), [])
    allx = [(x, 'display') for x in d] + [(x, 'export') for x in e]
    allx.sort(key=lambda t: -((t[0][2] - t[0][1]) if t[0][2] < 10**9 else 10**12))
    ex = '; '.join(f"{b.split('__', 1)[-1]} {mo} ({fmt(a, 3) if isinstance(a, float) else a} -> {fmt(c_, 3) if isinstance(c_, float) else c_})" for (b, a, c_), mo in allx[:4])
    w(f'| {cname} | {len(d)} | {len(e)} | {ex} |')
w()
w('Full lists:')
w()
for (cname, mode), lst in sorted(REG.items()):
    if len(lst) > 0 and not cname.startswith('triangles ('):
        w(f'- {cname}, {mode} ({len(lst)}): ' + '; '.join(f"{b.split('__', 1)[-1]} ({fmt(a, 3) if isinstance(a, float) else a} -> {fmt(c_, 3) if isinstance(c_, float) else c_})" for b, a, c_ in sorted(lst, key=lambda t: -((t[2] - t[1]) if t[2] < 10**9 else 10**12))[:40]))
w()

# time
w('## 4. Time')
w()
w('Wall time of the meshing call (plan + evaluate; median of 5 runs under 150 ms, else one run), one process per body and setting, one mesher at a time, release build.')
w()
w('| setting | mesher | bodies | median ms | p95 ms | p99 ms | slowest ms | sum s |')
w('|---|---|---|---|---|---|---|---|')
for mode in MODES:
    for l in LABELS:
        t = [r['ms'] for r in rows(l, mode)]
        w(f'| {mode} | {l} | {len(t)} | {st.median(t):.2f} | {q(t, .95):.1f} | {q(t, .99):.1f} | {max(t):.0f} | {sum(t) / 1000:.1f} |')
w()
w('Slowest bodies, export setting (ms):')
w()
w('| body | faces | ' + ' | '.join(LABELS) + ' | triangles (combined) |')
w('|---|---|' + '---|' * len(LABELS) + '---|')
rs = [(max(get(l, 'export', b)['ms'] for l in LABELS), b) for b in bodies if all(get(l, 'export', b) and get(l, 'export', b).get('ok') for l in LABELS)]
for _, b in sorted(rs, reverse=True)[:12]:
    w(f"| {b.split('__', 1)[-1][:60]} | {get('combined', 'export', b)['faces']} | " + ' | '.join(f"{get(l, 'export', b)['ms']:.0f}" for l in LABELS) + f" | {get('combined', 'export', b)['tris']:,} |")
w()
# cracks
w('Closed bodies (no boundary edge in the B-rep) with open edges in the mesh, one column per mesher: open edges (true gaps).')
w()
w('| body | setting | ' + ' | '.join(LABELS) + ' |')
w('|---|---|' + '---|' * len(LABELS))
seen = 0
for b in bodies:
    for mo in MODES:
        rr = [get(l, mo, b) for l in LABELS]
        if all(r and r.get('ok') for r in rr) and rr[0]['brep_boundary_edges'] == 0 and any(r['open_edges'] > 0 for r in rr):
            seen += 1; w(f"| {b.split('__', 1)[-1]} | {mo} | " + ' | '.join(f"{r['open_edges']} ({r['open_unsealed']})" for r in rr) + ' |')
if not seen: w('| none | | | | |')
w()
w('Bodies where any mesher exceeds the chord tolerance by more than 5 percent (max deviation / chord):')
w()
w('| body | setting | ' + ' | '.join(LABELS) + ' |')
w('|---|---|' + '---|' * len(LABELS))
lst = []
for b in bodies:
    for mo in MODES:
        v = [dev_ratio(get(l, mo, b)) for l in LABELS]
        if any(x and x > 1.05 for x in v): lst.append((max(x for x in v if x), b, mo, v))
for _, b, mo, v in sorted(lst, reverse=True)[:20]:
    w(f"| {b.split('__', 1)[-1]} | {mo} | " + ' | '.join('n/a' if x is None else f'{x:.2f}' for x in v) + ' |')
w()
open(OUTMD, 'w').write('\n'.join(out))
print(f'wrote {OUTMD}: {len(bodies)} bodies, results ' + ', '.join(f'{l}/{m}: {sum(1 for b in bodies if (l, m, b) in R)}' for l in LABELS for m in MODES))

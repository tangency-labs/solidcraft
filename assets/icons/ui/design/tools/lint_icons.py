#!/usr/bin/env python3
"""Rules every icon master in svg/ must satisfy (stdlib only). usage: lint_icons.py <design dir>"""
import sys, glob, os, re, tomllib, xml.etree.ElementTree as ET
import icon_palette
from pathlib import Path
d = sys.argv[1] if len(sys.argv) > 1 else os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
pal = tomllib.load(open(d + '/palette.toml', 'rb'))
light = {r.get('source', r['light']) for r in pal['roles'].values()}
OK_TAGS = {'svg', 'title', 'path', 'circle', 'defs', 'mask', 'clipPath', 'g'}
FAMILIES = {'model', 'small', 'flat', 'workspace', 'help'}
bad = []
def err(f, m): bad.append('%s: %s' % (os.path.relpath(f, d), m))
files = sorted(glob.glob(d + '/svg/*/*.svg'))
# Keep the collection free of generated previews, reports and authoring snapshots.
for path in Path(d).rglob('*'):
    if not path.is_file(): continue
    rel = path.relative_to(d).as_posix()
    allowed = (rel in {'palette.toml', 'aliases.toml', 'review.json', 'references.toml', 'requested_icons.txt'}
               or path.suffix == '.md'
               or (rel.startswith('svg/') and path.suffix == '.svg')
               or (path.parent in {Path(d) / 'tools', Path(d) / 'tools/author'} and path.suffix == '.py')
               or (path.parent == Path(d) / 'tools/review' and path.suffix in {'.html', '.css', '.js'}))
    if not allowed: err(str(path), 'generated or unsupported resource; keep temporary output under target/')
for f in files:
    fam, name = os.path.basename(os.path.dirname(f)), os.path.basename(f)[:-4]
    if fam not in FAMILIES: err(f, 'unknown family'); continue
    if not re.fullmatch(r'[a-z][a-z0-9_]*', name): err(f, 'file name must be an app icon id (lower_snake_case)')
    t = ET.parse(f).getroot(); ns = '{http://www.w3.org/2000/svg}'
    if t.get('viewBox') != '0 0 64 64': err(f, 'viewBox must be 0 0 64 64')
    for a in ('width', 'height'):
        if t.get(a): err(f, a + ' attribute on svg')
    ids = []
    def walk(e, mask):
        for c in e:
            tag = c.tag.replace(ns, '')
            if tag not in OK_TAGS: err(f, 'element <%s> not allowed' % tag)
            for a in ('transform', 'style', 'class'):
                if c.get(a): err(f, '%s attribute on <%s>' % (a, tag))
            if c.get('id'): ids.append(c.get('id'))
            if tag == 'path' and not re.fullmatch(r'[MLCQAZ0-9 .\-]+', c.get('d', '')): err(f, 'path data may use only absolute M L C Q A Z')
            m = mask or tag in ('mask', 'clipPath')
            for k in ('fill', 'stroke'):
                v = c.get(k)
                if v in (None, 'none') or v.startswith('url('): continue
                if m and v in ('white', 'black'): continue
                if v == 'currentColor':
                    continue
                if v.lower() not in light: err(f, '%s="%s" is not a palette colour' % (k, v))
                if fam == 'flat' and v.lower() != '#ffffff': err(f, 'flat icons are single colour: use currentColor, not %s' % v)
            if c.get('stroke') not in (None, 'none'):
                if not c.get('stroke-width'): err(f, 'stroke without stroke-width')
                if c.get('stroke-linejoin') != 'round' or c.get('stroke-linecap') != 'round': err(f, 'strokes need round joins and caps')
            if tag in ('path', 'circle') and c.get('fill') is None and c.get('stroke') is None and not m: err(f, '<%s> without paint' % tag)
            walk(c, m)
    walk(t, False)
    if len(ids) != len(set(ids)): err(f, 'duplicate ids')
# every small/ master has a model/ counterpart; aliases point at real masters
names = {os.path.basename(f)[:-4] for f in files if '/small/' not in f}
for f in files:
    if '/small/' in f and os.path.basename(f)[:-4] not in names: err(f, 'small variant without a master')
al = tomllib.load(open(d + '/aliases.toml', 'rb')).get('alias', {})
for a, t in al.items():
    if isinstance(t, dict):
        if t.get('accent') not in pal.get('accents', {}): err(d + '/aliases.toml', '%s: unknown accent %s' % (a, t.get('accent')))
        t = t.get('master')
    if t not in names: err(d + '/aliases.toml', '%s -> %s: no such master' % (a, t))
    if a in names: err(d + '/aliases.toml', '%s is both an alias and a master' % a)
primary = {}
for file in files:
    family, name = Path(file).parent.name, Path(file).stem
    if family != 'small': primary.setdefault(name, set()).add(family)
for name, families in primary.items():
    if len(families) > 1 and not (name == 'folder' and families == {'flat', 'model'}):
        err(d + '/svg', name + ': duplicate primary master across families')
references = d + '/references.toml'
if os.path.exists(references):
    for category in tomllib.load(open(references, 'rb')).get('category', {}).values():
        for name in category['icons']:
            if name not in names and name not in al: err(references, name + ': reference artwork is missing')
for message in icon_palette.validation_errors(pal, names | set(al)):
    err(d + '/palette.toml', message)
# the review page lies its icons on the real app surfaces: its colours must be the app's tokens
def app_tokens():
    src = open(os.path.join(d, '..', '..', '..', 'styles', 'ui', 'palettes.rs'), encoding='utf-8').read()
    lo, hi = src.index('pub const fn light'), src.index('pub const fn dark')
    light_src, dark_src = src[lo:hi], src[hi:]
    def pick(block, name, dark):
        pat = (r'\bt\.%s = Color32::from_rgb\((\d+), (\d+), (\d+)\)' if dark else r'\b%s: Color32::from_rgb\((\d+), (\d+), (\d+)\)') % name
        m = re.search(pat, block)
        return None if not m else '#%02x%02x%02x' % tuple(int(x) for x in m.groups())
    out = {}
    for name in ('toolbar', 'panel', 'border', 'text', 'text_dim', 'viewport_top'):
        out[name] = (pick(light_src, name, False), pick(dark_src, name, True) or pick(light_src, name, False))
    return out
css_path = os.path.join(d, 'tools', 'review', 'review.css')
if os.path.exists(css_path):
    try:
        tok = app_tokens()
        css = open(css_path, encoding='utf-8').read()
        want = {
            ':root{': {'--bg': tok['panel'][0], '--fg': tok['text'][0], '--card': tok['toolbar'][0], '--line': tok['border'][0], '--mut': tok['text_dim'][0]},
            'body.dark{': {'--bg': tok['viewport_top'][1], '--fg': tok['text'][1], '--card': tok['toolbar'][1], '--line': tok['border'][1], '--mut': tok['text_dim'][1]},
        }
        for block, values in want.items():
            line = next((l for l in css.split('\n') if l.startswith(block)), '')
            for var, hexv in values.items():
                m = re.search(re.escape(var) + r':(#[0-9a-fA-F]{6})', line)
                if not m or m.group(1).lower() != hexv:
                    err(css_path, '%s%s is %s, the app token is %s (palettes.rs)' % (block, var, m.group(1) if m else 'missing', hexv))
    except (OSError, ValueError) as e:
        err(css_path, 'cannot compare with palettes.rs: %s' % e)
# an icon accepted as final on the review page must not change (un-accept it there to work on it again)
import json, review_state
rp = d + '/review.json'
if os.path.exists(rp):
    fin = json.load(open(rp, encoding='utf-8')).get('final', {})
    for iid, want in (fin.items() if isinstance(fin, dict) else []):
        v = al.get(iid, iid); m = v.get('master') if isinstance(v, dict) else v
        if not want: continue
        # The review page's own hash, so the two can't disagree.
        if review_state.master_hash(m, d) != want: err(rp, 'final icon %s changed (master %s): un-accept it on the review page before editing it' % (iid, m))
print('\n'.join(bad) if bad else 'icons: %d masters, palette, naming and element rules ok' % len(files))
sys.exit(1 if bad else 0)

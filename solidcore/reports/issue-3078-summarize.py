"""Extract every attempted span/level; verify counts against independent profiles."""
import collections
import csv
import json
import re
import sys
from pathlib import Path

rows = []
case = None
span = None
level = None
counts = collections.defaultdict(collections.Counter)
profile_counts = {}
number = r'[-+]?(?:\d*\.?\d+(?:[eE][-+]?\d+)?|inf|NaN)'

for text in Path(sys.argv[1]).read_text().splitlines():
    if text.startswith('CASE '):
        case = text[5:]
    elif text.startswith('span begin'):
        counts[case]['spans'] += 1
        s = re.match(r'span begin s=\[(.*?),(.*?)\] target=(\S+) n=(\d+) law=(\S+)', text)
        span = dict(case=case, span=counts[case]['spans'], s0=s[1], s1=s[2], target=s[3])
    elif text.startswith('level begin'):
        m = re.match(r'level begin level=(\d+) pieces=(\d+)', text)
        level = dict(span, level=m[1], pieces=m[2], enclosures=0, failure='', bound='', lower='', min_den='', span_bound='')
        rows.append(level)
        counts[case]['L'+m[1]] += 1
    elif text.startswith('enclose s='):
        level['enclosures'] += 1
        counts[case]['enclosures'] += 1
    elif text.startswith(('enclose reject', 'arc reject')):
        level['failure'] += text + '; '
        counts[case][text.split('=')[0].split(' radii2')[0].split(' half')[0]] += 1
    elif text.startswith('level reject'):
        level['failure'] += text.split(' m4=')[0] + '; '
        if ' m4=' in text:
            m4, uv4 = text.split(' m4=')[1].split(' uv4=')
            level['m4_max'] = max(map(float, re.findall(number, m4)))
            level['uv4_max'] = max(map(float, re.findall(number, uv4)))
        counts[case][text.split(' m4=')[0]] += 1
    elif text.startswith('row bound'):
        den = float(re.search(r'den=(\S+)', text)[1])
        if level['min_den'] == '' or den < level['min_den']:
            level['min_den'] = den
            level['worst_row'] = text
    elif text.startswith('level bound='):
        m = re.match(r'level bound=(\S+) target=\S+ uv4=(.*?) m4=(.*)', text)
        level['bound'] = m[1]
        level['bound_excess'] = float(m[1]) - float(span['target'])
        if level['bound_excess'] > 0:
            level['failure'] += 'upper bound > target; '
        level['uv4_max'] = max(map(float, re.findall(number, m[2])))
        level['m4_max'] = max(map(float, re.findall(number, m[3])))
    elif text.startswith('level lower='):
        level['lower'] = text.split()[1].split('=')[1]
        level['last'] = text.split()[-1].split('=')[1]
    elif text.startswith('span end'):
        bound = re.search(r'bound=(\S+)', text)[1]
        for row in reversed(rows):
            if row['case'] != case or row['span'] != span['span']:
                break
            row['span_bound'] = bound
        counts[case]['span_ends'] += 1
        counts[case]['accepted' if float(bound) <= float(span['target']) else 'rejected'] += 1
        counts[case]['last_L'+level['level']] += 1
    elif text.startswith('{"type":"summary"'):
        record = json.loads(text)
        if record['name'] == 'blend::certificate::gn_enclose_over':
            profile_counts[case] = record['count']
    elif text == 'diagnostic capture overflow':
        raise AssertionError('incomplete capture')

for case, c in counts.items():
    assert c['spans'] == c['span_ends'], (case, c)
    assert c['enclosures'] == profile_counts[case], (case, c, profile_counts)
    print(case, json.dumps(dict(c), sort_keys=True))
fields = ['case', 'span', 's0', 's1', 'target', 'level', 'pieces', 'enclosures', 'failure', 'bound', 'bound_excess', 'lower', 'last', 'min_den', 'worst_row', 'm4_max', 'uv4_max', 'span_bound']
with Path(sys.argv[2]).open('w') as out:
    writer = csv.DictWriter(out, fieldnames=fields)
    writer.writeheader()
    writer.writerows(rows)
print('rows', len(rows))

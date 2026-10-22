#!/usr/bin/env python3
"""Compare the real store adapter with the pinned original SMLib executable."""
import argparse
import hashlib
import itertools
import json
import math
from pathlib import Path
import subprocess
import sys


def compare(reference, actual):
    if reference['ints'] != actual['ints']:
        return 'integer/state mismatch'
    if len(reference['values']) != len(actual['values']):
        return 'numeric field count mismatch'
    for index, (a, b) in enumerate(zip(reference['values'], actual['values'])):
        if not (math.isfinite(a) and math.isfinite(b)) or abs(a - b) > 1e-9 * max(abs(a), abs(b)):
            return f'numeric field {index}: C++={a!r}, Rust={b!r}'
    return None


def run(binary, args):
    result = subprocess.run([str(binary.resolve()), *map(str, args)], text=True, capture_output=True, timeout=60)
    record = {'exit_code': result.returncode, 'stdout': result.stdout, 'stderr': result.stderr}
    try:
        record['output'] = json.loads(result.stdout) if result.returncode == 0 else None
    except json.JSONDecodeError:
        record['output'] = None
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rust', type=Path, required=True)
    parser.add_argument('--cpp', type=Path, required=True)
    parser.add_argument('--cpp-library', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    options = parser.parse_args()
    cases = []
    sensitive = {'topology': 0, 'numeric': 0}
    numeric_eligible = 0
    for operation, kind, shared, scale in itertools.product(range(6), range(4), range(2), [0.001, 1.0, 1000.0]):
        for repetitions in ([1] if operation == 5 else [1, 2]):
            inputs = [[operation, kind, shared, scale, repetitions]]
            if shared == 0 and operation in (2, 3, 5):
                inputs.append([operation, kind, shared, scale, repetitions, 1])
            for args in inputs:
                cpp, rust = run(options.cpp, args), run(options.rust, args)
                case = {'input': args, 'cpp': cpp, 'rust': rust}
                if cpp['output'] is None or rust['output'] is None:
                    case.update(outcome='fail', reason='executable failed or output is not JSON')
                elif rust['output'].get('deferred'):
                    # This pinned point/wire corpus now completes on both sides.
                    # Preserve any future deferral as a failure of this regression.
                    case.update(outcome='fail', reason=rust['output']['deferred'])
                else:
                    reason = compare(cpp['output'], rust['output'])
                    case.update(outcome='fail' if reason else 'pass', reason=reason)
                    if reason is None:
                        topology = json.loads(json.dumps(rust['output']))
                        topology['ints'][5] += 1  # owning region count, not a status
                        sensitive['topology'] += compare(cpp['output'], topology) is not None
                        if rust['output']['values']:
                            numeric_eligible += 1
                            numeric = json.loads(json.dumps(rust['output']))
                            numeric['values'][0] += max(1., abs(numeric['values'][0])) * 1e-5
                            sensitive['numeric'] += compare(cpp['output'], numeric) is not None
                cases.append(case)

    passed = sum(c['outcome'] == 'pass' for c in cases)
    report = {
        'scope': 'Production CopyVertices, MergeBrep, scoped empty CopyFaces, PiecewiseMerge, OrientTrimmedSurfaces and Union entry points; point/wire inputs, shared/distinct contexts, reference attributes, source Copy callbacks and an independent merge-constructor context.',
        'qualification': 'Point/wire topology, vertex coordinates, intervals, attribute sharing and source copy callbacks are compared. Curve evaluation, completed solid Booleans, full HCR construction and native destructor behaviour are outside this probe.',
        'upstream': subprocess.check_output(['git', '-C', 'upstream', 'rev-parse', 'HEAD'], text=True).strip(),
        'relative_tolerance': 1e-9, 'absolute_floor': None,
        'cases_total': len(cases), 'passed': passed,
        'deferred': sum(c['outcome'] == 'deferred' for c in cases),
        'failed': sum(c['outcome'] == 'fail' for c in cases),
        'mutations_detected': sensitive,
        'numeric_mutation_eligible_cases': numeric_eligible,
        'values_compared': sum(len(c['rust']['output']['values']) for c in cases if c['outcome'] == 'pass'),
        'integers_compared': sum(len(c['rust']['output']['ints']) for c in cases if c['outcome'] == 'pass'),
        'source_sha256': {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in [Path(__file__), Path('crates/solidcore/examples/merge_adapter.rs'), Path('harness/cpp/merge_adapter_reference.cpp')]},
        'binaries': {name: {'path': str(path), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()} for name, path in [('rust', options.rust), ('cpp', options.cpp), ('cpp_library', options.cpp_library)]},
        'cases': cases,
    }
    options.output.parent.mkdir(parents=True, exist_ok=True)
    options.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: report[k] for k in ['cases_total', 'passed', 'deferred', 'failed', 'mutations_detected']}, indent=2))
    for case in cases:
        if case['outcome'] == 'fail':
            print(case['input'], case['reason'])
    required_comparisons = sum(c['input'][0] not in (3, 5) or (c['input'][0] == 3 and c['input'][1] == 3) for c in cases)
    return 0 if report['failed'] == 0 and passed >= required_comparisons and sensitive['topology'] == passed and sensitive['numeric'] == numeric_eligible else 1


if __name__ == '__main__':
    sys.exit(main())

'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const pc = require('./pr-checks.js');

const REPO = 'tangency-labs/solidcraft';

test('parseClosing finds closing keywords only', () => {
  assert.deepEqual(pc.parseClosing('Fixes #12', REPO), [12]);
  assert.deepEqual(pc.parseClosing('closes: #3\nResolved #4', REPO), [3, 4]);
  assert.deepEqual(pc.parseClosing('Fixes #5, #6 and #7', REPO), [5, 6, 7]);
  assert.deepEqual(pc.parseClosing('Fix #8', REPO), [8]);
  assert.deepEqual(pc.parseClosing('Refs #9\nSee #10\nrelated to #11', REPO), []);
  assert.deepEqual(pc.parseClosing('Prefixes #1 and suffix fixes#2', REPO), []);
  assert.deepEqual(pc.parseClosing(null, REPO), []);
});

test('parseClosing handles repo-qualified refs, comments and code', () => {
  assert.deepEqual(pc.parseClosing(`Fixes ${REPO}#20`, REPO), [20]);
  assert.deepEqual(pc.parseClosing('Fixes other/repo#21', REPO), []);
  assert.deepEqual(pc.parseClosing('<!-- Fixes #22 -->', REPO), []);
  assert.deepEqual(pc.parseClosing('```\nFixes #23\n```\nFixes #24', REPO), [24]);
  assert.deepEqual(pc.parseClosing('say `Fixes #25` to close', REPO), []);
});

test('parseDefaults', () => {
  assert.equal(pc.parseDefaults('x').status, 'missing');
  assert.equal(pc.parseDefaults('<!-- Changes a default: no -->').status, 'missing');
  assert.equal(pc.parseDefaults('Changes a default: no').status, 'no');
  assert.equal(pc.parseDefaults('- Changes a default: No.\r\nmore').status, 'no');
  assert.equal(pc.parseDefaults('**Changes a default:** no').status, 'no');
  assert.equal(pc.parseDefaults('Changes a default: <no | yes — <what>, old behaviour kept behind preference <name>>').status, 'malformed');
  assert.equal(pc.parseDefaults('Changes a default:').status, 'malformed');
  assert.equal(pc.parseDefaults('Changes a default: maybe').status, 'malformed');
  assert.equal(pc.parseDefaults('Changes a default: yes').status, 'malformed');
  assert.equal(pc.parseDefaults('Changes a default: yes - faster zoom').status, 'malformed');
});

test('parseDefaults yes forms', () => {
  for (const sep of ['—', '–', '-', '->', '→', ':']) {
    const r = pc.parseDefaults(`Changes a default: yes ${sep} zoom speed doubled, old behaviour kept behind preference viewport.zoom_speed`);
    assert.equal(r.status, 'yes', sep);
    assert.equal(r.preference, 'viewport.zoom_speed');
    assert.equal(r.detail, 'zoom speed doubled');
  }
  assert.equal(pc.parseDefaults('Changes a default: yes — x, old value kept behind preference `p`').status, 'yes');
  assert.equal(pc.parseDefaults('Changes a default: yes — x, old behaviour kept behind preference <name>').status, 'malformed');
});

test('isBot and isDocsOnly', () => {
  assert.ok(pc.isBot({ login: 'dependabot[bot]', type: 'Bot' }));
  assert.ok(pc.isBot({ login: 'solidcraft-generated[bot]' }));
  assert.ok(!pc.isBot({ login: 'alice', type: 'User' }));
  assert.ok(pc.isDocsOnly(['README.md', 'docs/a.png']));
  assert.ok(!pc.isDocsOnly(['README.md', 'crates/a.rs']));
  assert.ok(!pc.isDocsOnly([]));
});

const mk = (number, login, body, extra = {}) => ({
  number, state: 'open', user: { login }, body, title: `t${number}`,
  html_url: `https://github.com/${REPO}/pull/${number}`, labels: [], ...extra,
});

test('findDuplicates: lower number, other author, same issue', () => {
  const pulls = [
    mk(10, 'bob', 'Fixes #5'),
    mk(11, 'carol', 'Fixes #6'),
    mk(12, 'alice', 'Fixes #5'),
    mk(13, 'dan', 'Refs #5'),
    mk(14, 'erin', 'Closes #5', { draft: true }),
    mk(15, 'fay', 'Fixes #5', { labels: [{ name: 'not-a-duplicate' }] }),
    mk(30, 'gus', 'Fixes #5'),
    mk(16, 'hal', 'Fixes #5', { state: 'closed' }),
  ];
  const d = pc.findDuplicates({ number: 20, author: 'alice', issues: [5], pulls, repoFullName: REPO });
  assert.deepEqual(d.map((x) => x.number), [10, 14]);
  assert.ok(d[1].draft);
});

function fakes(pull, { pulls = [], files = [] } = {}) {
  const calls = [];
  const github = {
    paginate: async (fn) => (fn === 'list' ? pulls : files),
    rest: {
      pulls: { list: 'list', listFiles: 'files' },
      issues: {
        addLabels: async (a) => calls.push(['add', a.labels]),
        removeLabel: async (a) => calls.push(['remove', a.name]),
      },
    },
  };
  const failures = [];
  const core = { setFailed: (m) => failures.push(m), info() {}, notice() {} };
  const context = { repo: { owner: 'tangency-labs', repo: 'solidcraft' }, payload: { pull_request: pull } };
  return { github, core, context, calls, failures };
}

test('checkDuplicate fails with links, skips on label and bots', async () => {
  const me = mk(20, 'alice', 'Fixes #5');
  let f = fakes(me, { pulls: [mk(10, 'bob', 'Fixes #5')] });
  await pc.checkDuplicate(f);
  assert.equal(f.failures.length, 1);
  assert.match(f.failures[0], /pull\/10/);

  f = fakes({ ...me, labels: [{ name: 'not-a-duplicate' }] }, { pulls: [mk(10, 'bob', 'Fixes #5')] });
  await pc.checkDuplicate(f);
  assert.equal(f.failures.length, 0);

  f = fakes({ ...me, user: { login: 'dependabot[bot]', type: 'Bot' } }, { pulls: [mk(10, 'bob', 'Fixes #5')] });
  await pc.checkDuplicate(f);
  assert.equal(f.failures.length, 0);
});

test('checkDefaults fails when missing, passes docs-only and bots', async () => {
  const me = mk(20, 'alice', 'Fixes #5', { created_at: '2026-10-25T00:00:00Z' });
  let f = fakes(me, { files: [{ filename: 'crates/a.rs' }] });
  await pc.checkDefaults(f);
  assert.equal(f.failures.length, 1);

  f = fakes(me, { files: [{ filename: 'docs/a.md' }] });
  await pc.checkDefaults(f);
  assert.equal(f.failures.length, 0);

  f = fakes({ ...me, body: 'Changes a default: no' }, { files: [{ filename: 'crates/a.rs' }] });
  await pc.checkDefaults(f);
  assert.equal(f.failures.length, 0);
});

test('syncDefaultLabel adds and removes', async () => {
  const yes = 'Changes a default: yes — a, old behaviour kept behind preference p';
  let f = fakes(mk(1, 'a', yes));
  await pc.syncDefaultLabel(f);
  assert.deepEqual(f.calls, [['add', ['changes-default']]]);
  f = fakes(mk(1, 'a', 'Changes a default: no', { labels: [{ name: 'changes-default' }] }));
  await pc.syncDefaultLabel(f);
  assert.deepEqual(f.calls, [['remove', 'changes-default']]);
});

test('syncDefaultLabel notes a read-only token instead of failing', async () => {
  const yes = 'Changes a default: yes — a, old behaviour kept behind preference p';
  const f = fakes(mk(1, 'a', yes));
  const notes = [];
  f.core.notice = (m) => notes.push(m);
  f.github.rest.issues.addLabels = async () => { throw Object.assign(new Error('Resource not accessible by integration'), { status: 403 }); };
  await pc.syncDefaultLabel(f);
  assert.equal(notes.length, 1);
  f.github.rest.issues.addLabels = async () => { throw Object.assign(new Error('server error'), { status: 500 }); };
  await assert.rejects(pc.syncDefaultLabel(f));
});

test('checkDefaults only enforces from 2026-10-25', async () => {
  const files = [{ filename: 'crates/a.rs' }];
  const old = mk(20, 'alice', 'Fixes #5', { created_at: '2026-10-24T23:59:59Z' });
  let f = fakes(old, { files });
  assert.deepEqual(await pc.checkDefaults(f), { skipped: 'opened before enforcement' });
  assert.equal(f.failures.length, 0);
  f = fakes({ ...old, created_at: '2026-10-25T00:00:00Z' }, { files });
  await pc.checkDefaults(f);
  assert.equal(f.failures.length, 1);
});

test('hasPicture needs one a reader can see in the description', () => {
  assert.equal(pc.hasPicture('![tray](https://example.com/a.gif)'), true);
  assert.equal(pc.hasPicture('<img width="960" alt="x" src="https://github.com/user-attachments/assets/abc" />'), true);
  assert.equal(pc.hasPicture('<video src="https://example.com/a.mp4"></video>'), true);
  assert.equal(pc.hasPicture('https://github.com/user-attachments/assets/abc'), true);
  assert.equal(pc.hasPicture(''), false);
  assert.equal(pc.hasPicture(null), false);
  assert.equal(pc.hasPicture('The GIF is the change.'), false);
  assert.equal(pc.hasPicture('See docs/screenshots/box.gif'), false);
  assert.equal(pc.hasPicture('<!-- ![old](https://example.com/a.gif) -->'), false);
  assert.equal(pc.hasPicture('```\n![x](https://example.com/a.gif)\n```'), false);
  assert.equal(pc.hasPicture('`![x](https://example.com/a.gif)`'), false);
});

test('needsPicture covers the showcase and its media only', () => {
  assert.equal(pc.needsPicture(['docs/showcase.md']), true);
  assert.equal(pc.needsPicture(['crates/a.rs', 'docs/screenshots/a.gif']), true);
  assert.equal(pc.needsPicture(['docs/mcp.md', 'crates/a.rs']), false);
});

test('checkPicture fails a showcase PR with no picture, from 2026-11-02 on', async () => {
  const files = [{ filename: 'docs/showcase.md' }];
  const pr = mk(30, 'alice', 'The GIF is the change.', { created_at: '2026-11-02T00:00:00Z' });
  let f = fakes(pr, { files });
  assert.deepEqual(await pc.checkPicture(f), { shown: false });
  assert.equal(f.failures.length, 1);
  f = fakes({ ...pr, body: '![it](https://example.com/a.gif)' }, { files });
  assert.deepEqual(await pc.checkPicture(f), { shown: true });
  assert.equal(f.failures.length, 0);
  f = fakes({ ...pr, created_at: '2026-11-01T23:59:59Z' }, { files });
  assert.deepEqual(await pc.checkPicture(f), { skipped: 'opened before enforcement' });
  f = fakes(pr, { files: [{ filename: 'crates/a.rs' }] });
  assert.deepEqual(await pc.checkPicture(f), { skipped: 'nothing to show' });
  assert.equal(f.failures.length, 0);
});

test('isTestFile and isKernelSource', () => {
  for (const t of [
    'solidcore/crates/solidcore/tests/general_path_sweep.rs',
    'solidcore/crates/goldens/expected/blend.json',
    'solidcore/crates/goldens/src/units/fixtures/a.json',
    'crates/smlib/src/core/blend_tests.rs',
    'crates/smlib/src/tests.rs',
    'crates/doc/src/feature_walk_tests/x.rs',
    'tests/kernel-cases/fillets/a.json',
    'tests/oracle/85-plate-width-100/expected.json',
    'crates/ui-egui/src/snapshots/a.snap',
    'scripts/pr-checks/pr-checks.test.js',
  ]) assert.ok(pc.isTestFile(t), t);
  for (const t of ['solidcore/crates/solidcore/src/sweep/api_brep.rs', 'crates/smlib/src/core/blend.rs', 'crates/kernel/src/attest.rs'])
    assert.ok(!pc.isTestFile(t), t);
  assert.ok(pc.isKernelSource('solidcore/crates/solidcore/src/sweep/api_brep.rs'));
  assert.ok(pc.isKernelSource('crates/smlib/src/core/blend.rs'));
  assert.ok(!pc.isKernelSource('crates/smlib/src/core/blend_tests.rs'));
  assert.ok(!pc.isKernelSource('solidcore/plan/upstream-issues/WR-774.md'));
  assert.ok(!pc.isKernelSource('crates/kernel/src/ops.rs'));
});

test('testChangeReason', () => {
  assert.equal(pc.testChangeReason('Test change: the old expectation pinned the bug'), 'the old expectation pinned the bug');
  assert.equal(pc.testChangeReason('- **Test change:** radius was wrong in the fixture'), 'radius was wrong in the fixture');
  assert.equal(pc.testChangeReason('Test change:'), null);
  assert.equal(pc.testChangeReason('Test change: <reason>'), null);
  assert.equal(pc.testChangeReason('<!-- Test change: x -->'), null);
  assert.equal(pc.testChangeReason('```\nTest change: x\n```'), null);
  assert.equal(pc.testChangeReason(null), null);
});

test('changedTestLines ignores imports, comments, blank lines and moved lines', () => {
  const f = (patch) => ({ filename: 'x/tests/a.rs', status: 'modified', deletions: 9, patch });
  assert.equal(pc.changedTestLines(f('@@ -1,3 +1,3 @@\n-use solidcore::math::Vec3;\n-\n- // old note\n+use solidcore::math::Point3;')), 0);
  assert.equal(pc.changedTestLines(f('@@ -1,2 +1,2 @@\n-    assert!(ok);\n+        assert!(ok);')), 0);
  assert.equal(pc.changedTestLines(f('@@ -1,2 +1,2 @@\n-    let r = 2.0;\n+    let r = 1.5;')), 1);
  assert.equal(pc.changedTestLines(f('@@ -1,3 +1,0 @@\n-    /* obsolete\n-     * explanation\n-     */')), 0);
  // one re-added copy accounts for one of two removed assertions
  assert.equal(pc.changedTestLines(f('@@ -1,2 +1,1 @@\n-    assert!(ok);\n-    assert!(ok);\n+        assert!(ok);')), 1);
  assert.equal(pc.changedTestLines({ filename: 'x/tests/a.rs', status: 'modified', deletions: 3 }), 3);
  assert.equal(pc.changedTestLines({ filename: 'x/tests/a.rs', status: 'removed', deletions: 0 }), 1);
});

// The files of #14640 ("sweep: refuse a pipe whose radius reaches past a bend's centre"), which
// changed an existing test's input so the test passed.
const PR_14640 = [
  { filename: 'solidcore/crates/solidcore/src/sweep/api_brep.rs', status: 'modified', additions: 116, deletions: 2 },
  { filename: 'solidcore/crates/solidcore/tests/general_path_sweep.rs', status: 'modified', additions: 20, deletions: 3 },
  { filename: 'solidcore/plan/upstream-issues/WR-774--smapicreatepipesweep-builds-a-pipe-folded-past-a-bend.md', status: 'added', additions: 8, deletions: 0 },
];

test('checkTestChange flags a kernel fix that edits an existing test (#14640)', async () => {
  const pr = mk(14640, 'alice', 'Refuses the pipe.\n\nChanges a default: no');
  let f = fakes(pr, { files: PR_14640 });
  const r = await pc.checkTestChange(f);
  assert.deepEqual(r.edited, ['solidcore/crates/solidcore/tests/general_path_sweep.rs']);
  assert.equal(f.failures.length, 1);
  assert.match(f.failures[0], /general_path_sweep\.rs/);
  assert.match(f.failures[0], /Test change: <reason>/);
  f = fakes({ ...pr, body: `${pr.body}\nTest change: the old pipe radius reached past the bend centre` }, { files: PR_14640 });
  assert.equal((await pc.checkTestChange(f)).reason, 'the old pipe radius reached past the bend centre');
  assert.equal(f.failures.length, 0);
});

test('checkTestChange passes added tests, ledgers, non-kernel PRs and bots', async () => {
  const pr = mk(50, 'alice', 'Fix.');
  const added = [
    { filename: 'crates/smlib/src/core/blend.rs', status: 'modified', deletions: 4 },
    { filename: 'crates/smlib/src/core/blend_tests.rs', status: 'modified', deletions: 0 },
    { filename: 'solidcore/crates/goldens/expected/new_case.json', status: 'added', deletions: 0 },
    { filename: 'tests/KNOWN_FAILING_TESTS', status: 'modified', deletions: 1 },
  ];
  let f = fakes(pr, { files: added });
  assert.deepEqual(await pc.checkTestChange(f), { edited: [] });
  assert.equal(f.failures.length, 0);
  // a test file deleted outright counts
  f = fakes(pr, { files: [...added, { filename: 'crates/smlib/src/core/offset_tests.rs', status: 'removed', deletions: 40 }] });
  await pc.checkTestChange(f);
  assert.equal(f.failures.length, 1);
  // edited tests without kernel source: someone else's check
  f = fakes(pr, { files: [{ filename: 'crates/doc/src/tests.rs', status: 'modified', deletions: 9 }] });
  assert.deepEqual(await pc.checkTestChange(f), { skipped: 'no kernel source' });
  f = fakes({ ...pr, user: { login: 'dependabot[bot]', type: 'Bot' } }, { files: PR_14640 });
  assert.deepEqual(await pc.checkTestChange(f), { skipped: 'bot' });
});

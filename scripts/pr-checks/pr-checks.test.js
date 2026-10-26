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

test('syncDefaultLabel skips fork writes with a notice for adding and removing', async () => {
  for (const [body, labels] of [
    ['Changes a default: yes — a, old behaviour kept behind preference p', []],
    ['Changes a default: no', [{ name: 'changes-default' }]],
  ]) {
    const f = fakes(mk(1, 'a', body, { labels, head: { repo: { full_name: 'a/solidcraft' } } }));
    const notices = [];
    f.core.notice = (message) => notices.push(message);
    assert.deepEqual(await pc.syncDefaultLabel(f), { skipped: 'read-only fork token' });
    assert.deepEqual(f.calls, []);
    assert.equal(notices.length, 1);
    assert.match(notices[0], /read-only/);
  }
});

test('syncDefaultLabel keeps same-repository writes and propagates API errors', async () => {
  const f = fakes(mk(1, 'a', 'Changes a default: yes — a, old behaviour kept behind preference p', {
    head: { repo: { full_name: REPO.toUpperCase() } },
  }));
  await pc.syncDefaultLabel(f);
  assert.deepEqual(f.calls, [['add', ['changes-default']]]);
  for (const status of [403, 500]) {
    const error = Object.assign(new Error('API failed'), { status });
    f.github.rest.issues.addLabels = async () => { throw error; };
    await assert.rejects(pc.syncDefaultLabel(f), (actual) => actual === error);
  }
});

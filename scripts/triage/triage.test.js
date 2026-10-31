'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const triage = require('./triage.js');

const repo = { owner: 'tangency-labs', repo: 'solidcraft' };

// A fake github-script client that records writes (and list queries, in `reads`). `world`
// describes the repo.
function fake(world = {}) {
  const calls = [];
  const reads = [];
  const rec = (name) => async (args) => {
    calls.push([name, args]);
    return { data: {} };
  };
  const github = {
    paginate: async (fn, args) => (await fn(args)).data,
    rest: {
      repos: {
        getCollaboratorPermissionLevel: async ({ username }) => ({
          data: { permission: (world.perms || {})[username] || 'none' },
        }),
      },
      pulls: { listFiles: async () => ({ data: (world.files || []).map((filename) => ({ filename })) }) },
      reactions: { createForIssueComment: rec('react') },
      issues: {
        getLabel: async () => {
          if (world.labelMissing) throw Object.assign(new Error('nf'), { status: 404 });
          return { data: {} };
        },
        createLabel: rec('createLabel'),
        addLabels: rec('addLabels'),
        removeLabel: rec('removeLabel'),
        createComment: rec('comment'),
        listForRepo: async (args) => {
          reads.push(['listForRepo', args]);
          return { data: world.items || [] };
        },
        listEvents: async ({ issue_number }) => ({ data: (world.events || {})[issue_number] || [] }),
        listLabelsForRepo: async () => ({
          data: (world.labels || ['bug', 'enhancement', 'area:kernel', 'priority:P1', 'priority:P2', 'needs-triage']).map(
            (name) => ({ name }),
          ),
        }),
      },
    },
  };
  return { github, calls, reads };
}

const find = (calls, n) => calls.find((c) => c[0] === n);
const all = (calls, n) => calls.filter((c) => c[0] === n);
const labels = (names) => names.map((name) => ({ name }));

test('classifyIssue: in-app failure reports get bug and the area from the title', () => {
  assert.deepEqual(triage.classifyIssue('FILLET: operation failed', '## Actual behavior\nx'), ['bug', 'area:kernel']);
  assert.deepEqual(triage.classifyIssue('CREATE SKETCH: operation failed', ''), ['bug', 'area:sketch']);
});

test('classifyIssue: the report\'s own command is the fallback, not its history', () => {
  const body = '## Recent commands\n"sketch.line"\n## Settings and selection\n    [["file.export", {}]]\n## Environment\n';
  assert.deepEqual(triage.classifyIssue('Modeling: unexpected result', body), ['bug', 'area:io']);
  assert.deepEqual(triage.classifyIssue('Modeling: unexpected result', '## Recent commands\n"sketch.line"\n'), ['bug']);
});

test('classifyIssue: crashes are P1 bugs, the command named in the title gives the area', () => {
  assert.deepEqual(triage.classifyIssue('Crash: ended without a clean exit while running `solid.loft`', ''), [
    'bug',
    'priority:P1',
    'area:kernel',
  ]);
  assert.deepEqual(triage.classifyIssue('Joining 90 segments: caught panic at truck', ''), ['bug', 'priority:P1']);
});

test('classifyIssue: feature requests and unmatched text', () => {
  assert.deepEqual(triage.classifyIssue('Feature request: macOS please make use of menubar', ''), ['enhancement', 'area:ui']);
  assert.deepEqual(triage.classifyIssue('Refactor with types', 'words'), []);
  assert.deepEqual(triage.classifyIssue(undefined, undefined), []);
});

test('classifyPaths maps changed files to areas', () => {
  assert.deepEqual(
    triage.classifyPaths(['crates/kernel/src/a.rs', 'crates/geom/b.rs', 'crates/ui-egui/c.rs', 'README.md', 'Cargo.lock']),
    ['area:infra', 'area:kernel', 'area:ui'],
  );
  assert.deepEqual(triage.classifyPaths(['crates/engine/src/cmd/x.rs']), []);
});

test('parseTriage', () => {
  assert.deepEqual(triage.parseTriage('/triage area:kernel p1 -enhancement'), {
    add: ['area:kernel', 'priority:P1'],
    remove: ['enhancement'],
  });
  assert.deepEqual(triage.parseTriage('/triage'), { add: [], remove: [] });
  assert.equal(triage.parseTriage('/triaged'), null);
  assert.equal(triage.parseTriage('please /triage'), null);
  assert.equal(triage.parseTriage('/triage ' + 'bug '.repeat(50)).add.length, 10);
});

test('an opened issue gets its labels plus needs-triage, which is created if missing', async () => {
  const { github, calls } = fake({ labelMissing: true });
  const issue = { number: 5, state: 'open', title: 'SHELL: operation failed', body: '', labels: [] };
  assert.deepEqual(await triage.handleOpened({ github, context: { repo, payload: { issue } } }), [
    'bug',
    'area:kernel',
    'needs-triage',
  ]);
  assert.ok(find(calls, 'createLabel'));
  assert.deepEqual(find(calls, 'addLabels')[1].labels, ['bug', 'area:kernel', 'needs-triage']);
});

test('an opened issue keeps the labels it was filed with and skips needs-triage with a priority', async () => {
  const { github, calls } = fake();
  const issue = { number: 5, state: 'open', title: 'Crash: x', body: '', labels: labels(['bug']) };
  assert.deepEqual(await triage.handleOpened({ github, context: { repo, payload: { issue } } }), ['priority:P1']);
  assert.deepEqual(find(calls, 'addLabels')[1].labels, ['priority:P1']);
});

test('an opened PR is labelled from its paths and never gets needs-triage', async () => {
  const { github, calls } = fake({ files: ['crates/sketch/src/solve.rs'] });
  const pull_request = { number: 9, state: 'open', title: 'Sketch: thing', body: '', labels: [] };
  assert.deepEqual(await triage.handleOpened({ github, context: { repo, payload: { pull_request } } }), ['area:sketch']);
  assert.deepEqual(find(calls, 'addLabels')[1], { ...repo, issue_number: 9, labels: ['area:sketch'] });
});

test('a priority label clears needs-triage', async () => {
  const { github, calls } = fake();
  const issue = { number: 5, labels: labels(['needs-triage', 'priority:P2']) };
  assert.equal(await triage.handleLabeled({ github, context: { repo, payload: { issue, label: { name: 'priority:P2' } } } }), true);
  assert.equal(find(calls, 'removeLabel')[1].name, 'needs-triage');
  const other = fake();
  assert.equal(
    await triage.handleLabeled({ github: other.github, context: { repo, payload: { issue, label: { name: 'bug' } } } }),
    false,
  );
  assert.equal(other.calls.length, 0);
});

const comment = (body, login = 'carol', issueLabels = []) => ({
  repo,
  payload: {
    issue: { number: 5, labels: labels(issueLabels) },
    comment: { id: 1, body, user: { login, type: 'User' } },
  },
});

test('/triage by a maintainer sets labels, replaces the priority and clears needs-triage', async () => {
  const { github, calls } = fake({ perms: { carol: 'write' } });
  const ctx = comment('/triage area:kernel P1', 'carol', ['priority:P2', 'needs-triage', 'bug']);
  assert.equal(await triage.handleComment({ github, context: ctx }), 'triage');
  assert.deepEqual(all(calls, 'removeLabel').map((c) => c[1].name).sort(), ['needs-triage', 'priority:P2']);
  assert.deepEqual(find(calls, 'addLabels')[1].labels, ['area:kernel', 'priority:P1']);
  assert.equal(find(calls, 'react')[1].content, '+1');
});

test('/triage from a non-maintainer or with an unknown label changes nothing', async () => {
  let w = fake();
  assert.equal(await triage.handleComment({ github: w.github, context: comment('/triage bug', 'mallory') }), 'denied');
  assert.equal(find(w.calls, 'addLabels'), undefined);
  w = fake({ perms: { carol: 'admin' } });
  assert.equal(await triage.handleComment({ github: w.github, context: comment('/triage area:nope') }), 'unknown');
  assert.equal(find(w.calls, 'addLabels'), undefined);
  assert.match(find(w.calls, 'comment')[1].body, /area:nope/);
});

test('bots and other comments are ignored', async () => {
  const { github, calls } = fake();
  const bot = comment('/triage bug');
  bot.payload.comment.user = { login: 'x[bot]', type: 'Bot' };
  assert.equal(await triage.handleComment({ github, context: bot }), 'skip');
  assert.equal(await triage.handleComment({ github, context: comment('hello') }), 'skip');
  assert.equal(calls.length, 0);
});

test('backfill only touches unlabelled items, and the dry run writes nothing', async () => {
  const items = [
    { number: 1, title: 'EXTRUDE: operation failed', body: '', labels: [] },
    { number: 2, title: 'Already sorted', body: '', labels: labels(['bug']) },
    { number: 3, title: 'x', body: '', labels: [], pull_request: {} },
  ];
  let w = fake({ items, files: ['crates/io/src/step.rs'] });
  const plan = await triage.backfill({ github: w.github, context: { repo }, dryRun: true });
  assert.deepEqual(
    plan.map((p) => [p.number, p.labels]),
    [
      [1, ['bug', 'area:kernel', 'needs-triage']],
      [3, ['area:io']],
    ],
  );
  assert.equal(w.calls.length, 0);
  w = fake({ items, files: ['crates/io/src/step.rs'] });
  await triage.backfill({ github: w.github, context: { repo }, dryRun: false });
  assert.deepEqual(all(w.calls, 'addLabels').map((c) => c[1].issue_number), [1, 3]);
});

// Times relative to a daily run well after the bot started.
const NOW = new Date('2026-10-23T06:00:00Z');
const hoursAgo = (h) => new Date(NOW.getTime() - h * 60 * 60 * 1000).toISOString();
const failed = (number, extra = {}) => ({ number, title: 'EXTRUDE: operation failed', body: '', labels: [], created_at: hoursAgo(24), ...extra });
const ev = (event, name) => ({ event, label: { name } });

test('the daily run triages what its own run missed and leaves alone what someone triaged', async () => {
  const items = [
    failed(10),
    failed(11, { title: 'x', pull_request: {} }),
    // From the issue form (`bug`), or claimed (`in progress`): still never triaged.
    failed(12, { labels: labels(['bug']) }),
    failed(13, { labels: labels(['in progress']) }),
    // Triaged, then a person removed the labels: left alone.
    failed(14),
    // Triaged now, locked, too new (its own run may still be queued) or too old: left alone.
    failed(15, { labels: labels(['area:kernel']) }),
    failed(16, { locked: true }),
    failed(17, { created_at: hoursAgo(2) }),
    failed(18, { created_at: hoursAgo(5 * 24) }),
    // A person removed `bug`, which the form had set: everything but `bug`.
    failed(19),
  ];
  const events = {
    12: [ev('labeled', 'bug')],
    13: [ev('labeled', 'in progress')],
    14: [ev('labeled', 'area:kernel'), ev('labeled', 'needs-triage'), ev('unlabeled', 'area:kernel'), ev('unlabeled', 'needs-triage')],
    19: [ev('labeled', 'bug'), ev('unlabeled', 'bug')],
  };
  const { github, calls, reads } = fake({ items, events, files: ['crates/io/src/step.rs'] });
  const { healed, failed: errors, waiting } = await triage.heal({ github, context: { repo }, now: NOW });
  assert.deepEqual(healed, [
    { number: 10, labels: ['bug', 'area:kernel', 'needs-triage'] },
    { number: 11, labels: ['area:io'] },
    { number: 12, labels: ['area:kernel', 'needs-triage'] },
    { number: 13, labels: ['bug', 'area:kernel', 'needs-triage'] },
    { number: 19, labels: ['area:kernel', 'needs-triage'] },
  ]);
  assert.deepEqual(errors, []);
  assert.equal(waiting, 0);
  assert.deepEqual(all(calls, 'addLabels').map((c) => c[1].issue_number), [10, 11, 12, 13, 19]);
  const listed = find(reads, 'listForRepo')[1];
  assert.deepEqual([listed.state, listed.since, listed.sort, listed.direction], ['open', hoursAgo(4 * 24), 'created', 'desc']);
});

test('the daily run leaves items from before the bot existed to the manual backfill', async () => {
  const now = new Date('2026-10-19T06:00:00Z');
  const items = [
    failed(30, { created_at: '2026-10-18T15:00:00Z' }),
    failed(31, { created_at: '2026-10-18T08:00:00Z' }),
  ];
  const { github, reads } = fake({ items });
  const { healed } = await triage.heal({ github, context: { repo }, now });
  assert.deepEqual(healed.map((h) => h.number), [30]);
  assert.equal(find(reads, 'listForRepo')[1].since, '2026-10-18T09:17:21.000Z');
});

test('the daily run skips an item the rules give nothing, and writes nothing then', async () => {
  const { github, calls } = fake({ items: [failed(20, { title: 'x', pull_request: {} })], files: [] });
  assert.deepEqual(await triage.heal({ github, context: { repo }, now: NOW }), { healed: [], failed: [], waiting: 0 });
  assert.equal(all(calls, 'addLabels').length, 0);
});

test('the daily run looks at `max` items at most, in the order listed (newest first)', async () => {
  const items = [40, 41, 42, 43].map((n) => failed(n));
  // 40 was triaged before: it still counts as looked at.
  const { github, calls } = fake({ items, events: { 40: [ev('labeled', 'needs-triage')] } });
  const { healed, waiting } = await triage.heal({ github, context: { repo }, now: NOW, max: 2 });
  assert.deepEqual(healed.map((h) => h.number), [41]);
  assert.equal(waiting, 2);
  assert.deepEqual(all(calls, 'addLabels').map((c) => c[1].issue_number), [41]);
});

test('an API error on one item is reported and the others are still triaged', async () => {
  const { github, calls } = fake({ items: [failed(50), failed(51), failed(52)] });
  const listEvents = github.rest.issues.listEvents;
  github.rest.issues.listEvents = async (args) => {
    if (args.issue_number === 51) throw Object.assign(new Error('Server Error'), { status: 502 });
    return listEvents(args);
  };
  const { healed, failed: errors } = await triage.heal({ github, context: { repo }, now: NOW });
  assert.deepEqual(healed.map((h) => h.number), [50, 52]);
  assert.deepEqual(errors, [{ number: 51, error: 'Server Error' }]);
  assert.deepEqual(all(calls, 'addLabels').map((c) => c[1].issue_number), [50, 52]);
});

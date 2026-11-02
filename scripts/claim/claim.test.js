'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const claim = require('./claim.js');

const NOW = Date.parse('2026-10-16T12:00:00Z');
const H = 3600 * 1000;
const ago = (ms) => new Date(NOW - ms).toISOString();

// A fake github-script client that records writes. `world` describes the repo.
function fake(world = {}) {
  const calls = [];
  const rec = (name) => async (args) => {
    calls.push([name, args]);
    if (world.fail && world.fail[name]) throw Object.assign(new Error(name), { status: world.fail[name] });
    return { data: {} };
  };
  const github = {
    paginate: async (fn, args) => (await fn(args)).data,
    rest: {
      repos: {
        getCollaboratorPermissionLevel: async ({ username }) => {
          const permission = (world.perms || {})[username] || 'none';
          return { data: { permission } };
        },
        getCommit: async () => ({ data: { commit: { committer: { date: world.pushed || ago(H) } } } }),
      },
      pulls: { get: async () => ({ data: { head: { sha: 'abc' } } }) },
      reactions: { createForIssueComment: rec('react') },
      issues: {
        getLabel: async () => {
          if (world.labelMissing) throw Object.assign(new Error('nf'), { status: 404 });
          return { data: {} };
        },
        createLabel: rec('createLabel'),
        addLabels: rec('addLabels'),
        removeLabel: rec('removeLabel'),
        addAssignees: rec('addAssignees'),
        removeAssignees: rec('removeAssignees'),
        createComment: rec('comment'),
        listForRepo: async () => ({ data: world.issues || [] }),
        get: async ({ issue_number }) => {
          const found = (world.byNumber || {})[issue_number];
          if (!found) throw Object.assign(new Error('nf'), { status: 404 });
          return { data: found };
        },
        listEventsForTimeline: async ({ issue_number }) => ({ data: world.timelines?.[issue_number] || world.timeline || [] }),
      },
    },
  };
  return { github, calls };
}

const names = (calls) => calls.map((c) => c[0]);
const find = (calls, n) => calls.find((c) => c[0] === n);
const labeled = (at) => ({ event: 'labeled', label: { name: 'in progress' }, created_at: at });
const said = (body, login = 'github-actions[bot]') => ({ event: 'commented', body, user: { login } });
const pr = (number, login = 'other', state = 'open', extra = {}) => ({
  event: 'cross-referenced',
  source: { issue: {
    number, state, pull_request: {}, user: { login },
    title: 'Work (#7)', body: '', repository: { full_name: 'bherbruck/solidcraft' },
    ...extra,
  } },
});

function ctx(body, { assignees = [], labels = [], user = 'alice', extra = {} } = {}) {
  return {
    repo: { owner: 'bherbruck', repo: 'solidcraft' },
    payload: {
      issue: {
        number: 7,
        state: 'open',
        locked: false,
        updated_at: ago(1000 * H),
        assignees: assignees.map((login) => ({ login })),
        labels: labels.map((name) => ({ name })),
        ...extra,
      },
      comment: { id: 99, body, user: { login: user, type: 'User' } },
    },
  };
}

test('parseCommand', () => {
  assert.equal(claim.parseCommand('/claim'), 'claim');
  assert.equal(claim.parseCommand('  /claim please'), 'claim');
  assert.equal(claim.parseCommand('/unclaim'), 'unclaim');
  assert.equal(claim.parseCommand('/claimed'), null);
  assert.equal(claim.parseCommand('I would /claim this'), null);
  assert.equal(claim.parseCommand(undefined), null);
});

test('claim on a free issue assigns, labels, reacts and replies', async () => {
  const { github, calls } = fake({ labelMissing: true });
  assert.equal(await claim.handleComment({ github, context: ctx('/claim'), now: NOW }), 'claim');
  assert.deepEqual(find(calls, 'addAssignees')[1].assignees, ['alice']);
  assert.deepEqual(find(calls, 'addLabels')[1].labels, ['in progress']);
  assert.equal(find(calls, 'createLabel')[1].color, 'fbca04');
  assert.equal(find(calls, 'react')[1].content, '+1');
  assert.match(find(calls, 'comment')[1].body, /^Claimed by @alice\. .*draft PR referencing #7 within 72 h/);
});

test('claim still labels and replies when assignment is refused', async () => {
  const { github, calls } = fake({ fail: { addAssignees: 422 } });
  await claim.handleComment({ github, context: ctx('/claim'), now: NOW });
  assert.ok(find(calls, 'addLabels'));
  assert.match(find(calls, 'comment')[1].body, /@alice/);
});

test('claim by the current claimant is a quiet thumbs up', async () => {
  const { github, calls } = fake();
  await claim.handleComment({ github, context: ctx('/claim', { assignees: ['alice'], labels: ['in progress'] }), now: NOW });
  assert.deepEqual(names(calls), ['react']);
});

test('claim on an issue claimed by someone else names them and when it lapses', async () => {
  const { github, calls } = fake({ timeline: [labeled(ago(10 * H))] });
  await claim.handleComment({ github, context: ctx('/claim', { assignees: ['bob'], labels: ['in progress'] }), now: NOW });
  assert.equal(find(calls, 'react')[1].content, 'eyes');
  const body = find(calls, 'comment')[1].body;
  assert.match(body, /@bob/);
  assert.match(body, /2026-10-19 02:00 UTC/); // 10 h ago + 72 h
  assert.equal(find(calls, 'addAssignees'), undefined);
});

test('a lapsed claim is taken over', async () => {
  const { github, calls } = fake({ timeline: [labeled(ago(100 * H))] });
  await claim.handleComment({ github, context: ctx('/claim', { assignees: ['bob'], labels: ['in progress'] }), now: NOW });
  assert.deepEqual(find(calls, 'removeAssignees')[1].assignees, ['bob']);
  assert.deepEqual(find(calls, 'addAssignees')[1].assignees, ['alice']);
});

test('a claim with a recent PR is not taken over', async () => {
  const { github, calls } = fake({ timeline: [labeled(ago(100 * H)), pr(50, 'bob')], pushed: ago(2 * H) });
  await claim.handleComment({ github, context: ctx('/claim', { assignees: ['bob'], labels: ['in progress'] }), now: NOW });
  assert.equal(find(calls, 'addAssignees'), undefined);
});

test('a maintainer\'s claim is never taken over', async () => {
  const { github, calls } = fake({ timeline: [labeled(ago(500 * H))], perms: { carol: 'write' } });
  await claim.handleComment({ github, context: ctx('/claim', { assignees: ['carol'], labels: ['in progress'] }), now: NOW });
  assert.equal(find(calls, 'addAssignees'), undefined);
  assert.equal(find(calls, 'react')[1].content, 'eyes');
});

test('an unclaimed issue with someone else\'s open PR points at the PR', async () => {
  const { github, calls } = fake({ timeline: [pr(42, 'bob')] });
  await claim.handleComment({ github, context: ctx('/claim'), now: NOW });
  assert.equal(find(calls, 'addAssignees'), undefined);
  assert.match(find(calls, 'comment')[1].body, /#42/);
});

test('a body cross-reference does not block a comment claim', async () => {
  const { github, calls } = fake({
    timeline: [pr(42, 'bob', 'open', { title: 'Another fix (#8)', body: 'Not in this PR: #7.' })],
  });
  await claim.handleComment({ github, context: ctx('/claim'), now: NOW });
  assert.deepEqual(find(calls, 'addAssignees')?.[1].assignees, ['alice']);
  assert.match(find(calls, 'comment')[1].body, /^Claimed by @alice/);
});

test('bots, locked issues, closed issues, PRs and other comments are ignored', async () => {
  const { github, calls } = fake();
  const bot = ctx('/claim');
  bot.payload.comment.user = { login: 'dependabot[bot]', type: 'Bot' };
  assert.equal(await claim.handleComment({ github, context: bot, now: NOW }), 'skip');
  assert.equal(await claim.handleComment({ github, context: ctx('/claim', { extra: { locked: true } }), now: NOW }), 'skip');
  assert.equal(await claim.handleComment({ github, context: ctx('/claim', { extra: { state: 'closed' } }), now: NOW }), 'skip');
  assert.equal(await claim.handleComment({ github, context: ctx('/claim', { extra: { pull_request: {} } }), now: NOW }), 'skip');
  assert.equal(await claim.handleComment({ github, context: ctx('hello'), now: NOW }), 'skip');
  assert.equal(calls.length, 0);
});

test('unclaim by the claimant releases', async () => {
  const { github, calls } = fake();
  await claim.handleComment({ github, context: ctx('/unclaim', { assignees: ['alice'], labels: ['in progress'] }), now: NOW });
  assert.deepEqual(find(calls, 'removeAssignees')[1].assignees, ['alice']);
  assert.equal(find(calls, 'removeLabel')[1].name, 'in progress');
});

test('unclaim by a maintainer releases, by a stranger does not', async () => {
  let w = fake({ perms: { carol: 'admin' } });
  await claim.handleComment({ github: w.github, context: ctx('/unclaim', { user: 'carol', assignees: ['bob'], labels: ['in progress'] }), now: NOW });
  assert.ok(find(w.calls, 'removeLabel'));
  w = fake();
  await claim.handleComment({ github: w.github, context: ctx('/unclaim', { user: 'mallory', assignees: ['bob'], labels: ['in progress'] }), now: NOW });
  assert.equal(find(w.calls, 'removeLabel'), undefined);
  assert.equal(find(w.calls, 'react')[1].content, 'eyes');
});

test('unclaim by a claimant the bot could not assign releases, by a stranger does not', async () => {
  const timeline = [labeled(ago(5 * H)), said('Claimed by @alice through #50. The claim lapses after 14 days without pushes.')];
  let w = fake({ timeline });
  await claim.handleComment({ github: w.github, context: ctx('/unclaim', { labels: ['in progress'] }), now: NOW });
  assert.equal(find(w.calls, 'removeLabel')[1].name, 'in progress');
  assert.equal(find(w.calls, 'removeAssignees'), undefined);
  assert.equal(find(w.calls, 'react')[1].content, '+1');
  w = fake({ timeline });
  await claim.handleComment({ github: w.github, context: ctx('/unclaim', { user: 'mallory', labels: ['in progress'] }), now: NOW });
  assert.equal(find(w.calls, 'removeLabel'), undefined);
  assert.match(find(w.calls, 'comment')[1].body, /Only the person who claimed this/);
  w = fake({ timeline, perms: { carol: 'write' } });
  await claim.handleComment({ github: w.github, context: ctx('/unclaim', { user: 'carol', labels: ['in progress'] }), now: NOW });
  assert.ok(find(w.calls, 'removeLabel'));
});

test('only the bot\'s latest claim reply, before any release or assignment, names the claimant', async () => {
  const cases = [
    [said('Claimed by @mallory.', 'mallory')], // not the bot
    [said('Claimed by @mallory.'), { event: 'unlabeled', label: { name: 'in progress' } }, labeled(ago(H))], // released since
    [said('Claimed by @mallory.'), said('Claimed by @bob.')], // taken over
  ];
  for (const timeline of cases) {
    const w = fake({ timeline });
    await claim.handleComment({ github: w.github, context: ctx('/unclaim', { user: 'mallory', labels: ['in progress'] }), now: NOW });
    assert.equal(find(w.calls, 'removeLabel'), undefined);
  }
  const w = fake({ timeline: [said('Claimed by @mallory.')] }); // someone is assigned since
  await claim.handleComment({ github: w.github, context: ctx('/unclaim', { user: 'mallory', assignees: ['bob'], labels: ['in progress'] }), now: NOW });
  assert.equal(find(w.calls, 'removeLabel'), undefined);
});

test('unclaim on an owner-assigned issue works only for the owner', async () => {
  let w = fake();
  await claim.handleComment({ github: w.github, context: ctx('/unclaim', { user: 'carol', assignees: ['bherbruck'], labels: ['in progress'] }), now: NOW });
  assert.equal(find(w.calls, 'removeLabel'), undefined);
  w = fake();
  await claim.handleComment({ github: w.github, context: ctx('/unclaim', { user: 'bherbruck', assignees: ['bherbruck'], labels: ['in progress'] }), now: NOW });
  assert.ok(find(w.calls, 'removeLabel'));
});

const issue = (number, assignees = ['bob']) => ({
  number,
  locked: false,
  updated_at: ago(1000 * H),
  assignees: assignees.map((login) => ({ login })),
  labels: [{ name: 'in progress' }],
});
const repo = { owner: 'bherbruck', repo: 'solidcraft' };

test('sweep releases a claim older than 72 h with no PR', async () => {
  const { github, calls } = fake({ issues: [issue(7)], timeline: [labeled(ago(80 * H))] });
  assert.deepEqual(await claim.sweep({ github, context: { repo }, now: NOW }), [7]);
  assert.ok(find(calls, 'removeLabel'));
  assert.deepEqual(find(calls, 'removeAssignees')[1].assignees, ['bob']);
  assert.match(find(calls, 'comment')[1].body, /free again/);
});

test('sweep keeps a young claim and a claim with a fresh PR, using the latest label time', async () => {
  let w = fake({ issues: [issue(7)], timeline: [labeled(ago(100 * H)), labeled(ago(5 * H))] });
  assert.deepEqual(await claim.sweep({ github: w.github, context: { repo }, now: NOW }), []);
  w = fake({ issues: [issue(7)], timeline: [labeled(ago(500 * H)), pr(9)], pushed: ago(3 * 24 * H) });
  assert.deepEqual(await claim.sweep({ github: w.github, context: { repo }, now: NOW }), []);
});

test('sweep releases a claim whose PR has been idle for 14 days', async () => {
  const w = fake({ issues: [issue(7)], timeline: [labeled(ago(500 * H)), pr(9)], pushed: ago(15 * 24 * H) });
  assert.deepEqual(await claim.sweep({ github: w.github, context: { repo }, now: NOW }), [7]);
  assert.match(find(w.calls, 'comment')[1].body, /14 days/);
});

test('sweep ignores a closed PR', async () => {
  const w = fake({ issues: [issue(7)], timeline: [labeled(ago(80 * H)), pr(9, 'other', 'closed')] });
  assert.deepEqual(await claim.sweep({ github: w.github, context: { repo }, now: NOW }), [7]);
});

test('an incidental PR mention does not extend a claim past 72 hours', async () => {
  const w = fake({
    issues: [issue(7)],
    timeline: [labeled(ago(80 * H)), pr(9, 'bob', 'open', { title: 'Other work', body: 'See #7.' })],
    pushed: ago(H),
  });
  assert.deepEqual(await claim.sweep({ github: w.github, context: { repo }, now: NOW }), [7]);
  assert.match(find(w.calls, 'comment')[1].body, /No draft PR showed up within 72 h/);
});

test('sweep never touches owner- or maintainer-claimed issues', async () => {
  const w = fake({
    issues: [issue(1, ['bherbruck']), issue(2, ['carol']), issue(3, ['bob', 'carol'])],
    timeline: [labeled(ago(900 * H))],
    perms: { carol: 'write' },
  });
  assert.deepEqual(await claim.sweep({ github: w.github, context: { repo }, now: NOW }), []);
  assert.equal(w.calls.length, 0);
});

test('sweep is idempotent: a released issue is no longer listed', async () => {
  const world = { issues: [issue(7)], timeline: [labeled(ago(80 * H))] };
  const w = fake(world);
  assert.deepEqual(await claim.sweep({ github: w.github, context: { repo }, now: NOW }), [7]);
  world.issues = [];
  assert.deepEqual(await claim.sweep({ github: w.github, context: { repo }, now: NOW }), []);
});

test('referencedIssues reads the title and closing keywords, capped', () => {
  assert.deepEqual(claim.referencedIssues('Fillet: chain (#123)', 'Fixes #9, closes: #10, see #11'), [123, 9, 10]);
  assert.deepEqual(claim.referencedIssues('no refs', 'mentions #4 only'), []);
  assert.deepEqual(claim.referencedIssues('#1 #2 #3 #4 #5 #6 #7', ''), [1, 2, 3, 4, 5]);
  assert.deepEqual(claim.referencedIssues(undefined, null), []);
});

const openIssue = (number, assignees = [], labels = []) => ({
  number,
  state: 'open',
  locked: false,
  assignees: assignees.map((login) => ({ login })),
  labels: labels.map((name) => ({ name })),
});
const pullCtx = (number, title, body = '', login = 'alice') => ({
  repo,
  payload: { pull_request: { number, state: 'open', title, body, user: { login, type: 'User' } } },
});

test('a PR referencing a free issue claims it for the author', async () => {
  const { github, calls } = fake({ byNumber: { 7: openIssue(7) } });
  assert.deepEqual(await claim.handlePull({ github, context: pullCtx(50, 'Thing (#7)') }), [7]);
  assert.deepEqual(find(calls, 'addAssignees')[1], { ...repo, issue_number: 7, assignees: ['alice'] });
  assert.deepEqual(find(calls, 'addLabels')[1].labels, ['in progress']);
  assert.match(find(calls, 'comment')[1].body, /Claimed by @alice through #50/);
});

test('a PR on an issue someone else holds comments on the PR and changes nothing', async () => {
  const { github, calls } = fake({ byNumber: { 7: openIssue(7, ['bob'], ['in progress']) } });
  assert.deepEqual(await claim.handlePull({ github, context: pullCtx(50, 'Thing (#7)') }), []);
  assert.equal(find(calls, 'addAssignees'), undefined);
  const c = find(calls, 'comment')[1];
  assert.equal(c.issue_number, 50);
  assert.match(c.body, /@bob/);
});

test('the lowest-numbered open PR wins an unrecorded claim', async () => {
  const { github, calls } = fake({
    byNumber: { 7: openIssue(7) },
    timeline: [pr(45, 'carol'), pr(42, 'bob')],
  });
  assert.deepEqual(await claim.handlePull({ github, context: pullCtx(50, 'Thing (#7)') }), []);
  assert.match(find(calls, 'comment')[1].body, /#42 claimed #7 first/);
});

test('a PR mentioning an issue outside its claim does not block the actual fix', async () => {
  const { github, calls } = fake({
    byNumber: { 6608: openIssue(6608) },
    timeline: [pr(6381, 'bob', 'open', {
      title: 'Cylinder reference edges (#6380)',
      body: 'Closes #6380.\n\n## Not in this PR\n\nCanvas value boxes are tracked by #6608.',
    })],
  });
  assert.deepEqual(await claim.handlePull({ github, context: pullCtx(6661, 'Canvas boxes (#6608)') }), [6608]);
  assert.deepEqual(find(calls, 'addAssignees')[1].assignees, ['alice']);
  assert.equal(find(calls, 'comment')[1].issue_number, 6608);
});

test('a closing keyword in the older PR body still reserves the issue', async () => {
  const { github, calls } = fake({
    byNumber: { 7: openIssue(7) },
    timeline: [pr(42, 'bob', 'open', { title: 'Earlier fix', body: 'Resolves #7.' })],
  });
  assert.deepEqual(await claim.handlePull({ github, context: pullCtx(50, 'Thing (#7)') }), []);
  assert.match(find(calls, 'comment')[1].body, /#42 claimed #7 first/);
  assert.equal(find(calls, 'addAssignees'), undefined);
});

test('a PR in another repository cannot reserve a local issue with the same number', async () => {
  const { github, calls } = fake({
    byNumber: { 7: openIssue(7) },
    timeline: [pr(42, 'bob', 'open', {
      repository: { full_name: 'someone/other-project' },
      body: 'Fixes #7. Related: bherbruck/solidcraft#7.',
    })],
  });
  assert.deepEqual(await claim.handlePull({ github, context: pullCtx(50, 'Thing (#7)') }), [7]);
  assert.deepEqual(find(calls, 'addAssignees')[1].assignees, ['alice']);
});

test('workflow permissions allow duplicate PR replies and subsequent issue claims', async (t) => {
  const workflow = readFileSync(join(__dirname, '../../.github/workflows/claim.yml'), 'utf8');
  const permissions = workflow.match(/^permissions:\n((?:[ \t]+[^\n]*\n)+)/m)?.[1] || '';
  for (const [reason, world, warning] of [
    ['existing claimant', { byNumber: { 7: openIssue(7, ['bob'], ['in progress']) } }, /@bob/],
    ['older PR', { byNumber: { 7: openIssue(7) }, timelines: { 7: [pr(42, 'bob')] } }, /#42 claimed #7 first/],
  ]) {
    await t.test(reason, async () => {
      const { github, calls } = fake({ ...world, byNumber: { ...world.byNumber, 8: openIssue(8) } });
      // Both destinations use issues.createComment, but a PR comment needs PR write access.
      // Enforce the real workflow's token permissions instead of letting every fake write pass.
      const createComment = github.rest.issues.createComment;
      github.rest.issues.createComment = async (args) => {
        const scope = args.issue_number === 50 ? 'pull-requests' : 'issues';
        assert.match(permissions, new RegExp(`^  ${scope}: write(?:[ \\t]*(?:#.*)?)?$`, 'm'),
          `workflow token cannot comment on #${args.issue_number}: ${scope}: write is required`);
        return createComment(args);
      };
      assert.deepEqual(await claim.handlePull({ github, context: pullCtx(50, 'Thing (#7) (#8)') }), [8]);
      const comments = calls.filter(([name]) => name === 'comment').map(([, args]) => args);
      assert.deepEqual(comments.map((c) => c.issue_number), [50, 8]);
      assert.match(comments[0].body, warning);
      assert.match(comments[1].body, /Claimed by @alice through #50/);
      for (const name of ['addAssignees', 'addLabels']) {
        assert.deepEqual(calls.filter(([n]) => n === name).map(([, args]) => args.issue_number), [8]);
      }
    });
  }
});

test('the claimant\'s own PR just makes sure the label is there', async () => {
  const { github, calls } = fake({ byNumber: { 7: openIssue(7, ['alice']) } });
  assert.deepEqual(await claim.handlePull({ github, context: pullCtx(50, 'Thing (#7)') }), []);
  assert.deepEqual(names(calls), ['addLabels']);
});

test('PRs from bots, references to PRs, closed and missing issues are ignored', async () => {
  const w = fake({
    byNumber: { 7: { ...openIssue(7), pull_request: {} }, 8: { ...openIssue(8), state: 'closed' } },
  });
  assert.deepEqual(await claim.handlePull({ github: w.github, context: pullCtx(50, 'x (#7) (#8) (#9)') }), []);
  const bot = pullCtx(51, 'x (#1)');
  bot.payload.pull_request.user = { login: 'dependabot[bot]', type: 'Bot' };
  assert.deepEqual(await claim.handlePull({ github: w.github, context: bot }), []);
  assert.equal(w.calls.length, 0);
});

'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const paused = require('./kernel-paused.js');

const repo = { owner: 'tangency-labs', repo: 'solidcraft' };

// A fake github-script client that records writes. `world` describes the repo.
function fake(world = {}) {
  const calls = [];
  const rec = (name) => async (args) => {
    calls.push([name, args]);
    return { data: {} };
  };
  const github = {
    paginate: async (fn, args) => (await fn(args)).data,
    graphql: async (query, args) => {
      calls.push([query.trim().split(/[\s({]/)[0] === 'query' ? 'query' : query.match(/\{\s*(\w+)/)[1], args]);
      return { repository: { label: { id: 'L1' } } };
    },
    rest: {
      repos: {
        getCollaboratorPermissionLevel: async ({ username }) => ({ data: { permission: (world.perms || {})[username] || 'read' } }),
      },
      pulls: { listFiles: async () => ({ data: (world.files || []).map((filename) => ({ filename })) }) },
      issues: {
        getLabel: async () => {
          if (world.labelMissing) throw Object.assign(new Error('nf'), { status: 404 });
          return { data: {} };
        },
        createLabel: rec('createLabel'),
        addLabels: rec('addLabels'),
        createComment: rec('comment'),
      },
    },
  };
  return { github, calls };
}

const names = (calls) => calls.map((c) => c[0]);

test('kernel topics are named in titles; hole and shell are not', () => {
  for (const t of ['Fillet fails on a cone', 'Booleans: cut leaves a sliver', 'STEP import drops a face', 'Tessellation hangs']) {
    assert.ok(paused.isTopic(t), t);
  }
  for (const t of ['Hole dialog: diameter field', 'Shell command icon', 'Sketch: dimension text', 'Stepper widget']) {
    assert.ok(!paused.isTopic(t), t);
  }
});

test('a kernel issue is labelled, nothing more', async () => {
  const { github, calls } = fake({ labelMissing: true });
  const context = { repo, payload: { issue: { number: 5, title: 'Chamfer on a spline edge fails', body: '', user: { login: 'someone' } } } };
  await paused.handleOpened({ github, context });
  assert.deepEqual(names(calls), ['createLabel', 'addLabels']);
});

test('crashes, data loss and maintainers are left alone', async () => {
  for (const body of ['The app crashed when I filleted this', 'I lost my work after a boolean']) {
    const { github, calls } = fake();
    await paused.handleOpened({ github, context: { repo, payload: { issue: { number: 6, title: 'Fillet', body, user: { login: 'someone' } } } } });
    assert.deepEqual(calls, [], body);
  }
  const { github, calls } = fake({ perms: { owner: 'admin' } });
  await paused.handleOpened({ github, context: { repo, payload: { issue: { number: 7, title: 'Fillet', body: '', user: { login: 'owner' } } } } });
  assert.deepEqual(calls, []);
});

test('a pull request that changes the kernel is labelled, whatever its title', async () => {
  const { github, calls } = fake({ files: ['docs/a.md', 'crates/kernel/src/blend.rs'] });
  const pr = { number: 8, title: 'Speed up a hot path', body: '', draft: false, node_id: 'PR_1', user: { login: 'someone' } };
  await paused.handleOpened({ github, context: { repo, payload: { pull_request: pr } } });
  assert.deepEqual(names(calls), ['addLabels']);
});

test('a pull request outside the kernel is left alone', async () => {
  const { github, calls } = fake({ files: ['crates/ui-egui/src/dialogs.rs'] });
  const pr = { number: 9, title: 'Dialog spacing', body: '', draft: false, node_id: 'PR_2', user: { login: 'someone' } };
  await paused.handleOpened({ github, context: { repo, payload: { pull_request: pr } } });
  assert.deepEqual(calls, []);
});

test('a pull request with a kernel topic in its title but no kernel files is left alone', async () => {
  const { github, calls } = fake({ files: ['crates/ui-egui/src/dialogs.rs'] });
  const pr = { number: 10, title: 'Fillet dialog: a clearer radius field', body: '', draft: false, node_id: 'PR_3', user: { login: 'someone' } };
  await paused.handleOpened({ github, context: { repo, payload: { pull_request: pr } } });
  assert.deepEqual(calls, []);
});

test('a pull request that changes crates/geom is labelled', async () => {
  const { github, calls } = fake({ files: ['crates/geom/src/lib.rs'] });
  const pr = { number: 11, title: 'Tidy a helper', body: '', draft: false, node_id: 'PR_4', user: { login: 'someone' } };
  await paused.handleOpened({ github, context: { repo, payload: { pull_request: pr } } });
  assert.deepEqual(names(calls), ['addLabels']);
});

test('a kernel discussion is labelled', async () => {
  const { github, calls } = fake();
  const d = { number: 3, title: 'Ideas for better booleans', body: '', node_id: 'D_1', user: { login: 'someone' } };
  await paused.handleDiscussion({ github, context: { repo, payload: { discussion: d } } });
  assert.deepEqual(names(calls), ['query', 'addLabelsToLabelable']);
});

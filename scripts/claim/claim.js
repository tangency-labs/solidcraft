// The /claim bot's logic, used by .github/workflows/claim.yml through actions/github-script.
// Everything takes the github-script `github` client and `context`, so the tests can pass fakes.
'use strict';

const LABEL = 'in progress';
const LABEL_COLOR = 'fbca04';
const LABEL_DESCRIPTION = 'Someone is working on this';
const CLAIM_HOURS = 72;
const PR_IDLE_DAYS = 14;
const HOUR = 3600 * 1000;
const BOT = 'github-actions[bot]';

function parseCommand(body) {
  const text = String(body || '').trimStart();
  if (/^\/claim(\s|$)/.test(text)) return 'claim';
  if (/^\/unclaim(\s|$)/.test(text)) return 'unclaim';
  return null;
}

function fmt(date) {
  return new Date(date).toISOString().slice(0, 16).replace('T', ' ') + ' UTC';
}

function repoArgs(context) {
  return { owner: context.repo.owner, repo: context.repo.repo };
}

async function isMaintainer(github, context, login, cache) {
  if (login === context.repo.owner) return true;
  if (cache && cache.has(login)) return cache.get(login);
  let result = false;
  try {
    const { data } = await github.rest.repos.getCollaboratorPermissionLevel({
      ...repoArgs(context),
      username: login,
    });
    result =
      ['admin', 'write', 'maintain'].includes(data.permission) ||
      ['admin', 'maintain', 'write'].includes(data.role_name);
  } catch (e) {
    result = false;
  }
  if (cache) cache.set(login, result);
  return result;
}

async function ensureLabel(github, context) {
  try {
    await github.rest.issues.getLabel({ ...repoArgs(context), name: LABEL });
  } catch (e) {
    if (e.status !== 404) throw e;
    try {
      await github.rest.issues.createLabel({
        ...repoArgs(context),
        name: LABEL,
        color: LABEL_COLOR,
        description: LABEL_DESCRIPTION,
      });
    } catch (e2) {
      if (e2.status !== 422) throw e2; // created meanwhile
    }
  }
}

// When the label was last added, from the timeline; the issue's update time if it can't be found.
async function labeledAt(github, context, issue, timeline) {
  const events = timeline || (await getTimeline(github, context, issue.number));
  let at = null;
  for (const ev of events) {
    if (ev.event === 'labeled' && ev.label && ev.label.name === LABEL) at = ev.created_at;
  }
  return new Date(at || issue.updated_at).getTime();
}

function getTimeline(github, context, number) {
  return github.paginate(github.rest.issues.listEventsForTimeline, {
    ...repoArgs(context),
    issue_number: number,
    per_page: 100,
  });
}

// Cross-references discover candidates, not claims: even a comment or a "Not in this PR"
// mention creates one. Count only open PRs in this repo whose title/body actually claim the
// issue, using the same rule as handlePull. Timeline order is not PR-number order.
function linkedPulls(timeline, context, number) {
  const repo = `${context.repo.owner}/${context.repo.repo}`.toLowerCase();
  const seen = new Map();
  for (const ev of timeline) {
    const src = ev.event === 'cross-referenced' && ev.source && ev.source.issue;
    if (src && src.pull_request && src.state === 'open' &&
        src.repository?.full_name?.toLowerCase() === repo &&
        referencedIssues(src.title, src.body).includes(number)) {
      seen.set(src.number, src);
    }
  }
  return [...seen.values()].sort((a, b) => a.number - b.number);
}

// Who the bot's latest "Claimed by @user" reply names, unless the label came off after it.
// GitHub doesn't assign people who aren't collaborators, so for them this reply is the only record.
function recordedClaimant(timeline) {
  let who = null;
  for (const ev of timeline) {
    if (ev.event === 'unlabeled' && ev.label && ev.label.name === LABEL) who = null;
    const mine = ev.event === 'commented' && ev.user && ev.user.login === BOT;
    const m = mine && /^Claimed by @([A-Za-z0-9-]+)/.exec(ev.body || '');
    if (m) who = m[1];
  }
  return who;
}

async function lastPush(github, context, pr) {
  const { data } = await github.rest.pulls.get({ ...repoArgs(context), pull_number: pr.number });
  const { data: commit } = await github.rest.repos.getCommit({
    ...repoArgs(context),
    ref: data.head.sha,
  });
  return new Date(commit.commit.committer.date).getTime();
}

// Where a claimed issue stands: when it lapses, and whether it is free to release now.
async function claimStatus(github, context, issue, now) {
  const timeline = await getTimeline(github, context, issue.number);
  const since = await labeledAt(github, context, issue, timeline);
  const pulls = linkedPulls(timeline, context, issue.number);
  if (pulls.length === 0) {
    const expires = since + CLAIM_HOURS * HOUR;
    return { pulls, since, expires, release: now >= expires, reason: 'no PR' };
  }
  let newest = 0;
  for (const pr of pulls) newest = Math.max(newest, await lastPush(github, context, pr));
  const expires = newest + PR_IDLE_DAYS * 24 * HOUR;
  return { pulls, since, expires, release: now >= expires, reason: 'idle PR' };
}

async function react(github, context, content) {
  await github.rest.reactions.createForIssueComment({
    ...repoArgs(context),
    comment_id: context.payload.comment.id,
    content,
  });
}

async function reply(github, context, body) {
  await github.rest.issues.createComment({
    ...repoArgs(context),
    issue_number: context.payload.issue.number,
    body,
  });
}

function hasLabel(issue) {
  return (issue.labels || []).some((l) => (typeof l === 'string' ? l : l.name) === LABEL);
}

async function release(github, context, issue, assignees) {
  if (assignees.length) {
    await github.rest.issues.removeAssignees({
      ...repoArgs(context),
      issue_number: issue.number,
      assignees,
    });
  }
  try {
    await github.rest.issues.removeLabel({
      ...repoArgs(context),
      issue_number: issue.number,
      name: LABEL,
    });
  } catch (e) {
    if (e.status !== 404) throw e;
  }
}

async function claim(github, context, now) {
  const issue = context.payload.issue;
  const user = context.payload.comment.user.login;
  const n = issue.number;
  const cache = new Map();
  const assignees = (issue.assignees || []).map((a) => a.login);

  if (assignees.includes(user)) {
    await react(github, context, '+1'); // already theirs
    return;
  }

  // Someone else is assigned: theirs unless the claim has lapsed (never for a maintainer).
  let held = assignees.length > 0 || hasLabel(issue);
  let status = null;
  if (held) {
    let maintained = false;
    for (const a of assignees) maintained = maintained || (await isMaintainer(github, context, a, cache));
    if (!maintained) {
      status = await claimStatus(github, context, issue, now);
      if (status.release) held = false;
    }
    if (held) {
      await react(github, context, 'eyes');
      const who = assignees.length ? assignees.map((a) => '@' + a).join(', ') + ' has' : 'Someone has';
      const when = maintained || !status ? '' : ` The claim lapses on ${fmt(status.expires)} unless there's progress.`;
      await reply(github, context, `${who} claimed this already.${when}`);
      return;
    }
  } else {
    // Not claimed, but a draft PR may already be the claim (CONTRIBUTING.md, "Claim = a draft PR").
    const timeline = await getTimeline(github, context, n);
    const other = linkedPulls(timeline, context, n).find((p) => p.user && p.user.login !== user);
    if (other) {
      await react(github, context, 'eyes');
      await reply(github, context, `#${other.number} already covers this one.`);
      return;
    }
  }

  // Free (or lapsed): hand it to the commenter. Clear a lapsed claimant first.
  if (assignees.length) {
    await github.rest.issues.removeAssignees({ ...repoArgs(context), issue_number: n, assignees });
  }
  await ensureLabel(github, context);
  await github.rest.issues.addLabels({ ...repoArgs(context), issue_number: n, labels: [LABEL] });
  try {
    await github.rest.issues.addAssignees({
      ...repoArgs(context),
      issue_number: n,
      assignees: [user],
    });
  } catch (e) {
    // Assignment can be refused; the label and the reply naming them still stand.
  }
  await react(github, context, '+1');
  await reply(
    github,
    context,
    `Claimed by @${user}. Please open a draft PR referencing #${n} within ${CLAIM_HOURS} h; the claim lapses otherwise. \`/unclaim\` releases it.`,
  );
}

async function unclaim(github, context) {
  const issue = context.payload.issue;
  const user = context.payload.comment.user.login;
  const assignees = (issue.assignees || []).map((a) => a.login);
  if (!hasLabel(issue) && assignees.length === 0) {
    await react(github, context, '+1'); // nothing to release
    return;
  }
  if (assignees.includes(context.repo.owner) && user !== context.repo.owner) {
    await react(github, context, 'eyes'); // only the owner releases their own issues
    return;
  }
  let allowed = assignees.includes(user) || (await isMaintainer(github, context, user));
  if (!allowed && assignees.length === 0) {
    allowed = recordedClaimant(await getTimeline(github, context, issue.number)) === user;
  }
  if (!allowed) {
    await react(github, context, 'eyes');
    await reply(github, context, `Only the person who claimed this, or a maintainer, can release it, @${user}.`);
    return;
  }
  await release(github, context, issue, assignees);
  await react(github, context, '+1');
  await reply(github, context, `Released. This one is free again, so anyone can \`/claim\` it.`);
}

// The comment event.
async function handleComment({ github, context, now = Date.now() }) {
  const { issue, comment } = context.payload;
  if (!issue || issue.pull_request || !comment) return 'skip';
  if (issue.state !== 'open' || issue.locked) return 'skip';
  if (comment.user.type === 'Bot' || comment.user.login.endsWith('[bot]')) return 'skip';
  const cmd = parseCommand(comment.body);
  if (!cmd) return 'skip';
  if (cmd === 'claim') await claim(github, context, now);
  else await unclaim(github, context);
  return cmd;
}

// Issue numbers a PR claims: `#N` in its title (`<summary> (#N)`) and closing keywords in its body.
const MAX_REFS = 5;
function referencedIssues(title, body) {
  const found = new Set();
  for (const m of String(title || '').matchAll(/(?:^|[\s(])#(\d{1,7})\b/g)) found.add(Number(m[1]));
  const closing = /\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\s*:?\s+#(\d{1,7})\b/gi;
  for (const m of String(body || '').matchAll(closing)) found.add(Number(m[1]));
  return [...found].filter((n) => n > 0).slice(0, MAX_REFS);
}

// A pull request was opened (or its title or body edited): it claims each open issue it
// references that nobody holds yet, so a draft PR alone is enough (CONTRIBUTING.md).
async function handlePull({ github, context }) {
  const pull = context.payload.pull_request;
  if (!pull || pull.state !== 'open') return [];
  const author = pull.user && pull.user.login;
  if (!author || pull.user.type === 'Bot' || author.endsWith('[bot]')) return [];
  const claimed = [];
  for (const n of referencedIssues(pull.title, pull.body)) {
    if (n === pull.number) continue;
    let issue;
    try {
      ({ data: issue } = await github.rest.issues.get({ ...repoArgs(context), issue_number: n }));
    } catch (e) {
      if (e.status === 404 || e.status === 410) continue;
      throw e;
    }
    if (issue.pull_request || issue.state !== 'open' || issue.locked) continue;
    const assignees = (issue.assignees || []).map((a) => a.login);
    if (assignees.includes(author)) {
      if (!hasLabel(issue)) {
        await ensureLabel(github, context);
        await github.rest.issues.addLabels({ ...repoArgs(context), issue_number: n, labels: [LABEL] });
      }
      continue;
    }
    if (assignees.length || hasLabel(issue)) {
      const who = assignees.length ? assignees.map((a) => '@' + a).join(', ') : 'someone else';
      await github.rest.issues.createComment({
        ...repoArgs(context),
        issue_number: pull.number,
        body: `#${n} is already claimed by ${who}. If this PR duplicates their work, please close it or offer to help there.`,
      });
      continue;
    }
    // An older open PR for the issue wins (lowest number), even if it never recorded its claim.
    const timeline = await getTimeline(github, context, n);
    const older = linkedPulls(timeline, context, n).find((p) => p.number < pull.number && p.user && p.user.login !== author);
    if (older) {
      await github.rest.issues.createComment({
        ...repoArgs(context),
        issue_number: pull.number,
        body: `#${older.number} claimed #${n} first (the lowest-numbered open PR wins). Please close this one or offer to help there.`,
      });
      continue;
    }
    await ensureLabel(github, context);
    await github.rest.issues.addLabels({ ...repoArgs(context), issue_number: n, labels: [LABEL] });
    try {
      await github.rest.issues.addAssignees({ ...repoArgs(context), issue_number: n, assignees: [author] });
    } catch (e) {
      // Assignment can be refused; the label and the comment naming them still stand.
    }
    await github.rest.issues.createComment({
      ...repoArgs(context),
      issue_number: n,
      body: `Claimed by @${author} through #${pull.number}. The claim lapses after ${PR_IDLE_DAYS} days without pushes.`,
    });
    claimed.push(n);
  }
  return claimed;
}

// The daily sweep over open issues labelled `in progress`.
async function sweep({ github, context, now = Date.now() }) {
  const issues = await github.paginate(github.rest.issues.listForRepo, {
    ...repoArgs(context),
    state: 'open',
    labels: LABEL,
    per_page: 100,
  });
  const cache = new Map();
  const released = [];
  for (const issue of issues) {
    if (issue.pull_request || issue.locked) continue;
    const assignees = (issue.assignees || []).map((a) => a.login);
    let skip = false;
    for (const a of assignees) skip = skip || (await isMaintainer(github, context, a, cache));
    if (skip) continue;
    const status = await claimStatus(github, context, issue, now);
    if (!status.release) continue;
    await release(github, context, issue, assignees);
    await github.rest.issues.createComment({
      ...repoArgs(context),
      issue_number: issue.number,
      body:
        status.reason === 'no PR'
          ? `No draft PR showed up within ${CLAIM_HOURS} h, so this one is free again. Anyone can \`/claim\` it.`
          : `Nothing has been pushed for ${PR_IDLE_DAYS} days, so this one is free again. Anyone can \`/claim\` it.`,
    });
    released.push(issue.number);
  }
  return released;
}

module.exports = {
  LABEL,
  parseCommand,
  referencedIssues,
  isMaintainer,
  linkedPulls,
  claimStatus,
  handleComment,
  handlePull,
  sweep,
};

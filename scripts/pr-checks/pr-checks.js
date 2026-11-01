// Pull request checks for .github/workflows/pr-checks.yml (CONTRIBUTING.md, "Sending a change"):
//   1. duplicate: fails when a lower-numbered open PR already closes the same issue;
//   2. defaults: the template line `Changes a default: no | yes - <what>, old behaviour kept behind
//      preference <name>` must be present and well formed; `yes` earns the `changes-default` label.
// The parsing is pure and tested in scripts/pr-checks/pr-checks.test.js; the functions that talk to
// GitHub take the github-script `github` client and `context`, so the tests can pass fakes.
'use strict';

const SKIP_LABEL = 'not-a-duplicate';
const DEFAULT_LABEL = 'changes-default';
// PRs opened before this predate the template line; they pass the defaults check with a notice.
const DEFAULTS_ENFORCED_FROM = Date.parse('2026-10-25T00:00:00Z');
const BOT_LOGINS = new Set(['dependabot[bot]', 'github-actions[bot]', 'dependabot-preview[bot]']);

// Template comments and code blocks hold examples, not claims.
function stripNoise(body, { inlineCode = true } = {}) {
  const text = String(body || '')
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/```[\s\S]*?```/g, '')
    .replace(/~~~[\s\S]*?~~~/g, '');
  return inlineCode ? text.replace(/`[^`\n]*`/g, '') : text;
}

// Issue numbers after a closing keyword: Fixes #1, closes: #2, Resolved owner/repo#3.
// A list ("Fixes #1, #2") counts every number; a URL to another repo does not.
function parseClosing(body, repoFullName) {
  const text = stripNoise(body);
  const found = new Set();
  const re = /\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\b:?[ \t]+((?:(?:[\w.-]+\/[\w.-]+)?#\d+(?:[ \t]*(?:,|and|&)[ \t]*)?)+)/gi;
  for (const m of text.matchAll(re)) {
    for (const ref of m[1].matchAll(/(?:([\w.-]+\/[\w.-]+))?#(\d+)/g)) {
      if (ref[1] && repoFullName && ref[1].toLowerCase() !== repoFullName.toLowerCase()) continue;
      found.add(Number(ref[2]));
    }
  }
  return [...found].sort((a, b) => a - b);
}

// -> { status: 'missing' | 'malformed' | 'no' | 'yes', preference?, detail? }
function parseDefaults(body) {
  const m = stripNoise(body, { inlineCode: false }).replace(/\*\*/g, '').match(/^[ \t]*(?:[-*][ \t]+)?Changes a default:[ \t]*(.*?)[ \t]*$/im);
  if (!m) return { status: 'missing' };
  const value = m[1].replace(/\*\*/g, '').trim();
  if (/^no\.?$/i.test(value)) return { status: 'no' };
  const yes = value.match(/^yes\b[ \t]*(?:—|–|->|→|-{1,2}|:)[ \t]*(.+)$/i);
  if (yes) {
    const pref = yes[1].match(/^(.*?)[ ,;]*\bold (?:behaviou?r|value) kept behind (?:preference|pref|setting)[ \t]+`?([\w.:/-]+)`?\.?$/i);
    if (pref && pref[1].trim() && !/^<.*>$/.test(pref[2])) {
      return { status: 'yes', detail: pref[1].trim(), preference: pref[2] };
    }
  }
  return { status: 'malformed', detail: value };
}

const DEFAULTS_HELP =
  'Add one line to the description: `Changes a default: no`, or ' +
  '`Changes a default: yes — <what>, old behaviour kept behind preference <name>`.';

function isBot(user) {
  return !!user && (user.type === 'Bot' || BOT_LOGINS.has(user.login) || /\[bot\]$/.test(user.login || ''));
}

const DOC_FILE = /(^docs\/|\.md$|^LICENSE|^\.github\/ISSUE_TEMPLATE\/|^\.github\/pull_request_template\.md$)/;
function isDocsOnly(files) {
  return files.length > 0 && files.every((f) => DOC_FILE.test(f));
}

// Open PRs with a lower number than `number` that close one of `issues`, by someone else.
// `pulls` is a list of REST pull objects.
function findDuplicates({ number, author, issues, pulls, repoFullName }) {
  const out = [];
  for (const p of pulls) {
    if (p.number >= number || p.state !== 'open') continue;
    if (p.user && p.user.login === author) continue; // stacked work by the same author
    if ((p.labels || []).some((l) => l.name === SKIP_LABEL)) continue;
    const shared = parseClosing(p.body, repoFullName).filter((n) => issues.includes(n));
    if (shared.length) out.push({ number: p.number, title: p.title, url: p.html_url, draft: !!p.draft, issues: shared });
  }
  return out.sort((a, b) => a.number - b.number);
}

function hasLabel(pull, name) {
  return (pull.labels || []).some((l) => l.name === name);
}

async function checkDuplicate({ github, context, core }) {
  const pull = context.payload.pull_request;
  const repoFullName = `${context.repo.owner}/${context.repo.repo}`;
  if (isBot(pull.user)) return { skipped: 'bot' };
  if (hasLabel(pull, SKIP_LABEL)) return { skipped: SKIP_LABEL };
  const issues = parseClosing(pull.body, repoFullName);
  if (!issues.length) return { skipped: 'no closing keyword' };
  const pulls = await github.paginate(github.rest.pulls.list, {
    ...context.repo, state: 'open', per_page: 100,
  });
  const dups = findDuplicates({ number: pull.number, author: pull.user.login, issues, pulls, repoFullName });
  if (!dups.length) return { dups };
  const lines = dups.map(
    (d) => `  #${d.number}${d.draft ? ' (draft)' : ''} ${d.title}\n    ${d.url}  (closes ${d.issues.map((n) => '#' + n).join(', ')})`,
  );
  core.setFailed(
    `A lower-numbered open pull request already closes the same issue, and the lowest number wins:\n${lines.join('\n')}\n` +
      `Close this one or help on that one. If this is not a duplicate (a different fix that happens to name the same issue), ` +
      `change "Fixes" to "Refs" or ask a maintainer to add the \`${SKIP_LABEL}\` label.`,
  );
  return { dups };
}

async function checkDefaults({ github, context, core }) {
  const pull = context.payload.pull_request;
  if (isBot(pull.user)) return { skipped: 'bot' };
  if (!(Date.parse(pull.created_at) >= DEFAULTS_ENFORCED_FROM)) {
    core.notice('The "Changes a default:" line is only enforced on pull requests opened on or after 2026-10-25; this one predates it. Please add the line when you next push.');
    return { skipped: 'opened before enforcement' };
  }
  const files = (await github.paginate(github.rest.pulls.listFiles, {
    ...context.repo, pull_number: pull.number, per_page: 100,
  })).map((f) => f.filename);
  if (isDocsOnly(files)) return { skipped: 'docs only' };
  const result = parseDefaults(pull.body);
  if (result.status === 'missing') {
    core.setFailed(`The description has no "Changes a default:" line. ${DEFAULTS_HELP}`);
  } else if (result.status === 'malformed') {
    core.setFailed(`Could not read "Changes a default: ${result.detail}". ${DEFAULTS_HELP}`);
  }
  return result;
}

// Separate job (it needs write access, which fork PRs don't get): best effort.
async function syncDefaultLabel({ github, context, core }) {
  const pull = context.payload.pull_request;
  const { status } = parseDefaults(pull.body);
  const has = hasLabel(pull, DEFAULT_LABEL);
  const args = { ...context.repo, issue_number: pull.number };
  try {
    if (status === 'yes' && !has) {
      await github.rest.issues.addLabels({ ...args, labels: [DEFAULT_LABEL] });
    } else if (status !== 'yes' && has) {
      await github.rest.issues.removeLabel({ ...args, name: DEFAULT_LABEL });
    }
  } catch (e) {
    // A fork PR's token is read-only: say so instead of failing the check.
    if (e.status !== 403) throw e;
    core.notice(`Could not ${status === 'yes' ? 'add' : 'remove'} the ${DEFAULT_LABEL} label (read-only token); a maintainer sets it.`);
  }
  core.info(`Defaults line: ${status}`);
}

module.exports = {
  DEFAULTS_ENFORCED_FROM,  SKIP_LABEL, DEFAULT_LABEL, stripNoise, parseClosing, parseDefaults, isBot, isDocsOnly,
  findDuplicates, checkDuplicate, checkDefaults, syncDefaultLabel,
};

// The triage bot's logic, used by .github/workflows/triage.yml through actions/github-script.
// New issues and PRs get `area:*` and type labels from fixed rules; an issue nobody has given a
// priority yet carries `needs-triage`. Maintainers set labels with `/triage`. The rules only ever
// add labels, so a person's correction is never undone.
// Everything takes the github-script `github` client and `context`, so the tests can pass fakes.
'use strict';

const NEEDS_TRIAGE = 'needs-triage';
const NEEDS_TRIAGE_COLOR = 'ededed';
const NEEDS_TRIAGE_DESCRIPTION = 'no priority yet; a maintainer sorts it with /triage';
const MAX_TOKENS = 10;
const MAX_TEXT = 20000;

// Changed paths of a PR → area. First match wins per file.
const PATH_AREAS = [
  [/^crates\/sketch\//, 'area:sketch'],
  [/^crates\/(kernel|geom)\//, 'area:kernel'],
  [/^crates\/doc\//, 'area:doc'],
  [/^crates\/io\//, 'area:io'],
  [/^crates\/(render|gpu)\//, 'area:render'],
  [/^(crates\/(ui-egui|space-mouse|threedx-client)|apps\/(solidcraft|solidcraft-web))\//, 'area:ui'],
  [/^(\.github\/|xtask\/|scripts\/|tools\/|docs\/|packaging\/|[^/]+\.md$)/, 'area:infra'],
];

// Command id prefix (the in-app reports quote it) → area.
const COMMAND_AREAS = {
  sketch: 'area:sketch',
  solid: 'area:kernel',
  surface: 'area:kernel',
  sheet: 'area:kernel',
  plastic: 'area:kernel',
  construct: 'area:kernel',
  body: 'area:kernel',
  component: 'area:doc',
  joint: 'area:doc',
  parameters: 'area:doc',
  timeline: 'area:doc',
  occurrence: 'area:doc',
  motion: 'area:doc',
  contact: 'area:doc',
  explode: 'area:doc',
  file: 'area:io',
  render: 'area:render',
  appearance: 'area:render',
  view: 'area:ui',
  browser: 'area:ui',
  selection: 'area:ui',
};

// Words in an issue's title → area. Conservative: a miss leaves the issue for a person.
const WORD_AREAS = [
  [/\b(sketch|constraint|dimension|solver|spline)/i, 'area:sketch'],
  [/\b(fillet|chamfer|boolean|shell|loft|sweep|revolve|extrude|thread|hole|b-?rep|combine|offset|mirror|kernel|pattern)(s|ed|ing)?\b/i, 'area:kernel'],
  [/\b(step|iges|3mf|stl|gltf|dxf|obj|import|export)\b/i, 'area:io'],
  [/\b(render|gpu|wgpu|shader|opengl|vulkan|metal|appearance)\b/i, 'area:render'],
  [/\b(joint|assembly|timeline|parameter|component)/i, 'area:doc'],
  [/\b(toolbar|dialog|viewport|panel|button|shortcut|menu|window|space ?mouse|3dconnexion|spacenavd)/i, 'area:ui'],
];

const CRASH = /^\s*crash\b|\bpanic(ked)?\b|\bcaught panic\b/i;
const FEATURE = /^\s*\[?feature( request)?\]?\b/i;
// The in-app reports of a failed command: `<COMMAND>: operation failed` (older ones also
// `Modeling: unexpected result`). Other in-app reports carry an `## Actual behavior` section.
const IN_APP_FAILURE = /:\s*(operation failed|unexpected result)\b/i;

function repoArgs(context) {
  return { owner: context.repo.owner, repo: context.repo.repo };
}

// The text under a `## <name>` heading (`###` in issue forms), up to the next heading.
function section(body, name) {
  const lines = body.split('\n');
  const start = lines.findIndex((l) => new RegExp(`^#{2,3}\\s*${name}\\s*$`, 'i').test(l.trim()));
  if (start < 0) return '';
  const rest = lines.slice(start + 1);
  const end = rest.findIndex((l) => /^#{2,3}\s/.test(l));
  return (end < 0 ? rest : rest.slice(0, end)).join('\n');
}

function labelNames(item) {
  return (item.labels || []).map((l) => (typeof l === 'string' ? l : l.name));
}

function hasPriority(names) {
  return names.some((n) => n.startsWith('priority:'));
}

// Labels for an issue from its title and body.
function classifyIssue(title, body) {
  const t = String(title || '').slice(0, 500);
  const b = String(body || '').slice(0, MAX_TEXT);
  const labels = new Set();
  if (CRASH.test(t)) {
    labels.add('bug');
    labels.add('priority:P1'); // a crash loses work (AGENTS.md, "Never crash")
  } else if (IN_APP_FAILURE.test(t) || /^##\s*(behavior|actual behavior)\b/im.test(b)) {
    labels.add('bug');
  } else if (FEATURE.test(t)) {
    labels.add('enhancement');
  }
  // The title names the feature; the command quoted in the report's own section is the fallback
  // (a report's history quotes earlier commands too, so the rest of the body is not read).
  let area = null;
  for (const [re, a] of WORD_AREAS) {
    if (re.test(t)) {
      area = a;
      break;
    }
  }
  if (!area) {
    const own = `${t}\n${section(b, 'Settings and selection')}\n${section(b, 'Command')}`;
    const m = /["`]([a-z_]+)\.[a-z_][a-z_.]*["`]/.exec(own);
    area = (m && COMMAND_AREAS[m[1]]) || null;
  }
  if (area) labels.add(area);
  return [...labels];
}

// Labels for a PR from its changed paths.
function classifyPaths(paths) {
  const labels = new Set();
  for (const p of paths) {
    for (const [re, area] of PATH_AREAS) {
      if (re.test(p)) {
        labels.add(area);
        break;
      }
    }
  }
  return [...labels].sort();
}

async function ensureLabel(github, context, name, color, description) {
  try {
    await github.rest.issues.getLabel({ ...repoArgs(context), name });
  } catch (e) {
    if (e.status !== 404) throw e;
    try {
      await github.rest.issues.createLabel({ ...repoArgs(context), name, color, description });
    } catch (e2) {
      if (e2.status !== 422) throw e2; // created meanwhile
    }
  }
}

async function pullPaths(github, context, number) {
  const files = await github.paginate(github.rest.pulls.listFiles, {
    ...repoArgs(context),
    pull_number: number,
    per_page: 100,
  });
  return files.map((f) => f.filename);
}

// The labels the rules would add to an issue or PR that it doesn't already carry.
async function proposal(github, context, item) {
  const current = labelNames(item);
  let wanted;
  if (item.pull_request) {
    wanted = classifyPaths(await pullPaths(github, context, item.number));
  } else {
    wanted = classifyIssue(item.title, item.body);
    if (!hasPriority(current) && !wanted.some((l) => l.startsWith('priority:'))) wanted.push(NEEDS_TRIAGE);
  }
  return wanted.filter((l) => !current.includes(l));
}

async function apply(github, context, number, labels) {
  if (!labels.length) return;
  if (labels.includes(NEEDS_TRIAGE)) {
    await ensureLabel(github, context, NEEDS_TRIAGE, NEEDS_TRIAGE_COLOR, NEEDS_TRIAGE_DESCRIPTION);
  }
  await github.rest.issues.addLabels({ ...repoArgs(context), issue_number: number, labels });
}

// issues.opened / pull_request_target.opened.
async function handleOpened({ github, context }) {
  const issue = context.payload.issue;
  const pull = context.payload.pull_request;
  const item = issue || (pull && { ...pull, pull_request: {} });
  if (!item || item.state !== 'open') return [];
  const labels = await proposal(github, context, item);
  await apply(github, context, item.number, labels);
  return labels;
}

// issues.labeled: a priority settles triage.
async function handleLabeled({ github, context }) {
  const issue = context.payload.issue;
  const label = context.payload.label;
  if (!issue || !label || !label.name.startsWith('priority:')) return false;
  if (!labelNames(issue).includes(NEEDS_TRIAGE)) return false;
  try {
    await github.rest.issues.removeLabel({ ...repoArgs(context), issue_number: issue.number, name: NEEDS_TRIAGE });
  } catch (e) {
    if (e.status !== 404) throw e;
  }
  return true;
}

// `/triage area:kernel P1 bug -enhancement`: tokens name existing labels; `P1` is short for
// `priority:P1` and replaces any other priority; a leading `-` removes.
function parseTriage(body) {
  const line = String(body || '').trimStart().split('\n')[0];
  const m = /^\/triage(\s.*)?$/.exec(line.trimEnd());
  if (!m) return null;
  const add = [];
  const remove = [];
  for (const raw of (m[1] || '').trim().split(/\s+/).filter(Boolean).slice(0, MAX_TOKENS)) {
    const minus = raw.startsWith('-');
    let name = minus ? raw.slice(1) : raw;
    if (/^p[0-3]$/i.test(name)) name = 'priority:' + name.toUpperCase();
    if (name) (minus ? remove : add).push(name);
  }
  return { add, remove };
}

async function isMaintainer(github, context, login) {
  if (login === context.repo.owner) return true;
  try {
    const { data } = await github.rest.repos.getCollaboratorPermissionLevel({ ...repoArgs(context), username: login });
    return ['admin', 'write', 'maintain'].includes(data.permission);
  } catch (e) {
    return false;
  }
}

async function react(github, context, content) {
  await github.rest.reactions.createForIssueComment({
    ...repoArgs(context),
    comment_id: context.payload.comment.id,
    content,
  });
}

// issue_comment.created.
async function handleComment({ github, context }) {
  const { issue, comment } = context.payload;
  if (!issue || !comment || comment.user.type === 'Bot' || comment.user.login.endsWith('[bot]')) return 'skip';
  const cmd = parseTriage(comment.body);
  if (!cmd) return 'skip';
  if (!(await isMaintainer(github, context, comment.user.login))) {
    await react(github, context, 'eyes');
    return 'denied';
  }
  const known = new Set(
    (await github.paginate(github.rest.issues.listLabelsForRepo, { ...repoArgs(context), per_page: 100 })).map((l) => l.name),
  );
  const unknown = [...cmd.add, ...cmd.remove].filter((n) => !known.has(n));
  if (unknown.length) {
    await react(github, context, 'confused');
    await github.rest.issues.createComment({
      ...repoArgs(context),
      issue_number: issue.number,
      body: `No such label: ${unknown.map((n) => '`' + n + '`').join(', ')}. Nothing was changed.`,
    });
    return 'unknown';
  }
  const current = labelNames(issue);
  const remove = new Set(cmd.remove);
  if (cmd.add.some((n) => n.startsWith('priority:'))) {
    for (const n of current) if (n.startsWith('priority:') && !cmd.add.includes(n)) remove.add(n);
  }
  if (cmd.add.length || remove.size) remove.add(NEEDS_TRIAGE);
  for (const name of remove) {
    if (!current.includes(name)) continue;
    await github.rest.issues.removeLabel({ ...repoArgs(context), issue_number: issue.number, name });
  }
  const add = cmd.add.filter((n) => !current.includes(n));
  if (add.length) await github.rest.issues.addLabels({ ...repoArgs(context), issue_number: issue.number, labels: add });
  await react(github, context, '+1');
  return 'triage';
}

// Workflow dispatch: the rules over every open issue and PR with no labels at all. With `dryRun`
// it changes nothing and returns what it would do.
async function backfill({ github, context, dryRun = true }) {
  const items = await github.paginate(github.rest.issues.listForRepo, {
    ...repoArgs(context),
    state: 'open',
    per_page: 100,
  });
  const plan = [];
  for (const item of items) {
    if (labelNames(item).length || item.locked) continue;
    const labels = await proposal(github, context, item);
    plan.push({ number: item.number, pull: Boolean(item.pull_request), title: item.title, labels });
    if (!dryRun) await apply(github, context, item.number, labels);
  }
  return plan;
}

module.exports = {
  NEEDS_TRIAGE,
  classifyIssue,
  classifyPaths,
  ensureLabel,
  isMaintainer,
  labelNames,
  section,
  parseTriage,
  handleOpened,
  handleLabeled,
  handleComment,
  backfill,
};

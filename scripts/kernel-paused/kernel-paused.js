'use strict';
// While a new geometry kernel is evaluated, new issues and discussions about kernel work, and new
// pull requests that change the kernel directories, get the `kernel-paused` label, so a maintainer
// finds them and answers in person (CONTRIBUTING.md, "Kernel work is paused"). Reports of crashes
// and data loss are left alone, and so is anyone who can push to the repository.

const LABEL = 'kernel-paused';
const LABEL_COLOR = 'b60205';
const LABEL_DESCRIPTION = 'Kernel work is paused while a new geometry kernel is evaluated';

// Kernel operations named in the title of an issue or discussion. "Hole" and "shell" are left out
// on purpose: they name UI work too. A pull request is judged by its files, never by its title.
const TOPIC =
  /\b(booleans?|fillets?|chamfers?|offsets?|sweeps?|lofts?|threads?|step|stp|iges|igs|tessellat\w*|kernel|truck|b-?rep)\b/i;
// Crashes and lost work come first whatever the kernel is.
const URGENT = /\b(crash\w*|panic\w*|data loss|lost (my )?work|corrupt\w*)\b/i;
// What a pull request changes that makes it kernel work.
const KERNEL_PATHS = ['crates/kernel/', 'crates/geom/', 'vendor/truck'];

function isTopic(title) {
  return TOPIC.test(title || '');
}

function isUrgent(title, body) {
  return URGENT.test(`${title || ''}\n${body || ''}`);
}

function touchesKernel(files) {
  return files.some((f) => KERNEL_PATHS.some((p) => f.startsWith(p)));
}

// Whether the author can push to the repository (maintainers and their agents are not paused).
async function canPush({ github, context }, username) {
  try {
    const { data } = await github.rest.repos.getCollaboratorPermissionLevel({ ...context.repo, username });
    return ['admin', 'maintain', 'write'].includes(data.permission);
  } catch {
    return false;
  }
}

async function ensureLabel({ github, context }) {
  try {
    await github.rest.issues.getLabel({ ...context.repo, name: LABEL });
  } catch (e) {
    if (e.status !== 404) throw e;
    await github.rest.issues.createLabel({ ...context.repo, name: LABEL, color: LABEL_COLOR, description: LABEL_DESCRIPTION });
  }
}

// A new issue or pull request: returns what was done, for the log.
async function handleOpened({ github, context }) {
  const item = context.payload.pull_request || context.payload.issue;
  if (!item) return 'nothing to do';
  const pull = Boolean(context.payload.pull_request);
  if (isUrgent(item.title, item.body)) return `#${item.number}: a crash or data-loss report, left alone`;
  if (await canPush({ github, context }, item.user.login)) return `#${item.number}: by a maintainer, left alone`;
  let kernel;
  if (pull) {
    const files = await github.paginate(github.rest.pulls.listFiles, { ...context.repo, pull_number: item.number, per_page: 100 });
    kernel = touchesKernel(files.map((f) => f.filename));
  } else {
    kernel = isTopic(item.title);
  }
  if (!kernel) return `#${item.number}: not kernel work`;
  await ensureLabel({ github, context });
  await github.rest.issues.addLabels({ ...context.repo, issue_number: item.number, labels: [LABEL] });
  return `#${item.number}: labelled ${LABEL}`;
}

// A new discussion.
async function handleDiscussion({ github, context }) {
  const d = context.payload.discussion;
  if (!d) return 'nothing to do';
  if (!isTopic(d.title) || isUrgent(d.title, d.body)) return `discussion #${d.number}: left alone`;
  if (await canPush({ github, context }, d.user.login)) return `discussion #${d.number}: by a maintainer, left alone`;
  await ensureLabel({ github, context });
  const { repository } = await github.graphql(
    'query($owner: String!, $repo: String!, $name: String!) { repository(owner: $owner, name: $repo) { label(name: $name) { id } } }',
    { ...context.repo, name: LABEL },
  );
  if (repository && repository.label) {
    await github.graphql('mutation($id: ID!, $labels: [ID!]!) { addLabelsToLabelable(input: {labelableId: $id, labelIds: $labels}) { clientMutationId } }', {
      id: d.node_id,
      labels: [repository.label.id],
    });
  }
  return `discussion #${d.number}: labelled ${LABEL}`;
}

module.exports = { LABEL, isTopic, isUrgent, touchesKernel, handleOpened, handleDiscussion };

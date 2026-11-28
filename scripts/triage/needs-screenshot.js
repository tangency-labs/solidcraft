// App reports need the reporter to paste or drag the screenshot into GitHub's issue form.
// Ask once on issues.opened; the marker also prevents duplicate comments on workflow retries.
'use strict';

const { ensureLabel, labelNames } = require('./triage.js');

const LABEL = 'needs-screenshot';
const MARKER = '<!-- solidcraft-needs-screenshot -->';
const IMAGE = /!\[[^\]\n]*\]\s*(?:\(|\[)|<img\b|https:\/\/(?:github\.com\/(?:user-attachments\/|[^/\s]+\/[^/\s]+\/assets\/)|user-images\.githubusercontent\.com\/)/i;

async function handleOpened({ github, context }) {
  const issue = context.payload.issue;
  if (!issue || issue.state !== 'open' || issue.pull_request || issue.locked) return false;
  const body = String(issue.body || '').replace(/<!--[\s\S]*?-->/g, '');
  if (!/^##\s+Visual context\s*$/im.test(body) || IMAGE.test(body) || labelNames(issue).includes(LABEL)) return false;
  const args = { owner: context.repo.owner, repo: context.repo.repo, issue_number: issue.number };
  const comments = await github.paginate(github.rest.issues.listComments, { ...args, per_page: 100 });
  if (comments.some((comment) => comment.user && comment.user.type === 'Bot' && String(comment.body || '').includes(MARKER))) return false;
  await ensureLabel(github, context, LABEL, 'd4c5f9', 'waiting on the reporter to attach a screenshot');
  await github.rest.issues.addLabels({ ...args, labels: [LABEL] });
  await github.rest.issues.createComment({
    ...args,
    body: `${MARKER}\nPlease paste the screenshot into this issue (Ctrl+V, or Cmd+V on macOS), or drag the saved screenshot onto the issue form.`,
  });
  return true;
}

module.exports = { handleOpened };

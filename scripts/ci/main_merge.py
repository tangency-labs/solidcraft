#!/usr/bin/env python3
"""Did this pull request's change already pass CI, so this run need not test it again?

    python3 -I scripts/ci/main_merge.py --base BASE_SHA --head HEAD_SHA --repo OWNER/NAME \
        --pr NUMBER --head-repo OWNER/NAME --head-ref BRANCH [--run-id ID --attempt N]

Prints `carried=<url of the passing run>` when the change passed already, else `carried=`. It
passed already when any of these holds:
  - this very commit passed in another run of this pull request with the test shards (a label
    added again, approved-for-ci after a pass, a reopen, ready for review again);
  - this run's previous attempt passed with the shards ("Re-run all jobs" after a pass);
  - a commit of this pull request with the same tree (`git rev-parse HEAD^{tree}`, the same files
    exactly) passed with the shards: a rebase or a squash that changed nothing, a commit message
    reworded;
  - it is only a merge of main into a change that passed, when all of these hold:
  - HEAD is a merge commit whose second parent is on main (an ancestor of BASE): "Merge branch
    'main'", or GitHub's Update branch button;
  - the pull request's own change is the same before and after the merge (equal `git patch-id
    --verbatim`, whitespace included), so the merge resolved no conflict and brought nothing into
    the change itself;
  - a ci.yml run of this pull request (by number, or for a fork, where GitHub lists no pull
    request on the run, by head repository and branch) for the change before the merge passed
    with the test shards. When that change was
    itself only a merge of main (a run that carried, its shards skipped), the walk goes on down
    the first parents, through at most MAX_CHAIN merges, to the run where the shards ran.

Then the run needs only the repository checks: the change was tested, and main tests the merged
result after it lands. Any doubt (an error, an API failure, an unexpected shape) prints
`carried=` and the full checks run. Reads GH_TOKEN from the environment for the API.
"""
import argparse
import json
import os
import subprocess
import sys
import urllib.parse
import urllib.request

API = "https://api.github.com"
SHARD_PREFIX = "Native Linux tests"
# Consecutive merges of main followed back to a run whose shards ran.
MAX_CHAIN = 10


def git(*args):
    return subprocess.run(["git", *args], capture_output=True, text=True, check=True).stdout


def patch_id(a, b):
    """The stable patch id of `git diff a b` ("" for an empty diff)."""
    diff = subprocess.run(["git", "diff", "--no-renames", a, b], capture_output=True, check=True).stdout
    if not diff.strip():
        return ""
    # --verbatim: --stable ignores whitespace, which matters in strings and indentation.
    out = subprocess.run(["git", "patch-id", "--verbatim"], input=diff, capture_output=True, check=True).stdout
    return out.split()[0].decode() if out.split() else ""


def merge_of_main(base, head):
    """The first parent of HEAD when HEAD only merges main into an unchanged change, else None."""
    parents = git("rev-list", "--parents", "-n", "1", head).split()[1:]
    if len(parents) != 2:
        return None
    change, merged = parents
    if subprocess.run(["git", "merge-base", "--is-ancestor", merged, base]).returncode != 0:
        return None
    fork_point = git("merge-base", change, merged).strip()
    before, after = patch_id(fork_point, change), patch_id(merged, head)
    return change if before and before == after else None


def merge_chain(base, head):
    """The changes HEAD carries from, nearest first: the first parent of each consecutive merge of
    main that left the change's own patch as it was (each one equals the next, so all equal HEAD's)."""
    chain, cur = [], head
    for _ in range(MAX_CHAIN):
        change = merge_of_main(base, cur)
        if change is None:
            break
        chain.append(change)
        cur = change
    return chain


def get(url, token):
    req = urllib.request.Request(url, headers={"Accept": "application/vnd.github+json", "Authorization": f"Bearer {token}"})
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.load(r)


def same_pull_request(run, pr, head_repo, head_ref):
    """Whether a workflow run belongs to this pull request, not another one with the same head."""
    numbers = [p.get("number") for p in run.get("pull_requests") or []]
    if numbers:
        return pr in numbers
    return (run.get("head_repository") or {}).get("full_name") == head_repo and run.get("head_branch") == head_ref


def passing_run(repo, sha, token, pr, head_repo, head_ref):
    """URL of a successful ci.yml run of this pull request at `sha` whose test shards all passed."""
    runs = get(f"{API}/repos/{repo}/actions/workflows/ci.yml/runs?head_sha={sha}&event=pull_request&status=success&per_page=10", token)
    for run in runs.get("workflow_runs", []):
        if not same_pull_request(run, pr, head_repo, head_ref):
            continue
        jobs = get(f"{API}/repos/{repo}/actions/runs/{run['id']}/jobs?per_page=100", token).get("jobs", [])
        shards = [j for j in jobs if j.get("name", "").startswith(SHARD_PREFIX)]
        if shards and all(j.get("conclusion") == "success" for j in shards):
            return run.get("html_url", "")
    return ""


def previous_attempt(repo, run_id, attempt, token):
    """URL of this run's previous attempt when it passed with the test shards, else ""."""
    if attempt < 2:
        return ""
    prev = get(f"{API}/repos/{repo}/actions/runs/{run_id}/attempts/{attempt - 1}", token)
    if prev.get("conclusion") != "success":
        return ""
    jobs = get(f"{API}/repos/{repo}/actions/runs/{run_id}/attempts/{attempt - 1}/jobs?per_page=100", token).get("jobs", [])
    shards = [j for j in jobs if j.get("name", "").startswith(SHARD_PREFIX)]
    if shards and all(j.get("conclusion") == "success" for j in shards):
        return prev.get("html_url", "")
    return ""


def tree_of(repo, sha, token):
    """The tree of `sha`: from the local clone, or the API for a commit the clone lacks
    (a force-pushed one)."""
    try:
        return git("rev-parse", f"{sha}^{{tree}}").strip()
    except subprocess.CalledProcessError:
        return get(f"{API}/repos/{repo}/commits/{sha}", token).get("commit", {}).get("tree", {}).get("sha", "")


# Earlier heads of the pull request looked at for the same tree (each may cost an API call or two).
MAX_TREES = 5


def same_tree_run(repo, head, token, pr, head_repo, head_ref):
    """URL of a passing run of an earlier head of this pull request with HEAD's exact tree."""
    tree = git("rev-parse", f"{head}^{{tree}}").strip()
    runs = get(f"{API}/repos/{repo}/actions/workflows/ci.yml/runs?branch={urllib.parse.quote(head_ref, safe='')}&event=pull_request&status=success&per_page=30", token)
    seen = set()
    for run in runs.get("workflow_runs", []):
        sha = run.get("head_sha", "")
        if sha == head or sha in seen or not same_pull_request(run, pr, head_repo, head_ref):
            continue
        seen.add(sha)
        if len(seen) > MAX_TREES:
            break
        if tree_of(repo, sha, token) == tree:
            url = passing_run(repo, sha, token, pr, head_repo, head_ref)
            if url:
                return sha, url
    return "", ""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", required=True)
    ap.add_argument("--head", required=True)
    ap.add_argument("--repo", required=True)
    ap.add_argument("--pr", required=True, type=int)
    ap.add_argument("--head-repo", required=True)
    ap.add_argument("--head-ref", required=True)
    ap.add_argument("--run-id", default="")
    ap.add_argument("--attempt", type=int, default=1)
    a = ap.parse_args()
    carried = ""
    try:
        token = os.environ.get("GH_TOKEN", "")
        if token and a.run_id:
            carried = previous_attempt(a.repo, a.run_id, a.attempt, token)
            if carried:
                print(f"attempt {a.attempt - 1} of this run passed: {carried}", file=sys.stderr)
        if token and not carried:
            carried = passing_run(a.repo, a.head, token, a.pr, a.head_repo, a.head_ref)
            if carried:
                print(f"{a.head} already passed: {carried}", file=sys.stderr)
        if token and not carried:
            sha, carried = same_tree_run(a.repo, a.head, token, a.pr, a.head_repo, a.head_ref)
            if carried:
                print(f"{a.head} has the tree of {sha}, which passed: {carried}", file=sys.stderr)
        for change in merge_chain(a.base, a.head) if token and not carried else []:
            carried = passing_run(a.repo, change, token, a.pr, a.head_repo, a.head_ref)
            if carried:
                print(f"{a.head} merges main into {change}, which passed: {carried}", file=sys.stderr)
                break
    except (subprocess.CalledProcessError, OSError, ValueError, KeyError) as e:
        print(f"not carried ({e})", file=sys.stderr)
        carried = ""
    print(f"carried={carried}")


if __name__ == "__main__":
    main()

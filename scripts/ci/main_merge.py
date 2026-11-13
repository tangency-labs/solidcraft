#!/usr/bin/env python3
"""Is this pull request push only a merge of main into a change that already passed CI?

    python3 -I scripts/ci/main_merge.py --base BASE_SHA --head HEAD_SHA --repo OWNER/NAME \
        --pr NUMBER --head-repo OWNER/NAME --head-ref BRANCH

Prints `carried=<url of the passing run>` when all of these hold, else `carried=`:
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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", required=True)
    ap.add_argument("--head", required=True)
    ap.add_argument("--repo", required=True)
    ap.add_argument("--pr", required=True, type=int)
    ap.add_argument("--head-repo", required=True)
    ap.add_argument("--head-ref", required=True)
    a = ap.parse_args()
    carried = ""
    try:
        token = os.environ.get("GH_TOKEN", "")
        for change in merge_chain(a.base, a.head) if token else []:
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

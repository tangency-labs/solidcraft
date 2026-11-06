# Git-backed Cloud Saves: scenario checklist

This is the acceptance checklist for the Git storage workflow. Desktop standalone
connections use standard Git; GitHub connections use the shared GitHub API adapter
on desktop and in the browser. Each row identifies its expected behavior and the
focused regression group that exercises it. Transport smoke checks exercise the
actual desktop application against isolated SSH and authenticated HTTPS servers.

## Repository and branch state

| Scenario | Expected behavior | Coverage |
|---|---|---|
| Empty standalone checkout, empty remote | First Save creates and publishes one root commit. | G1, transport smoke |
| Empty checkout, local revisions only | First Save creates one local root commit. | G1 |
| Empty checkout, remote populated after connection | Sync fast-forwards to the first remote revision and preserves staged work. | G1 |
| Empty GitHub API repository at connection time | Initialize once through guided setup, then save ordinary design revisions. | API setup tests |
| Existing checkout, existing remote | Save commits only its captured files and confirms publication. | G2 |
| Existing checkout, empty push destination or missing branch | Publish the existing history and saved revision to the chosen branch. | G3 |
| Custom branch containing slashes | Read and publish exactly that branch. | G1, G4 |
| SHA-1 and SHA-256 object formats | Derive tree/commit identities using the repository's Git object format. | G1 |
| Linked worktree | Use its branch, index and Git lock paths; preserve the other worktree. | G4 |
| Wrong branch or detached HEAD | Pause publication and retain the capture. | G5 |
| Merge, cherry-pick, revert, rebase, bisect or index lock | Pause until the external operation finishes. | G5 |
| Another SolidCraft publisher owns the checkout | Serialize publication; a cancelled waiter retains its save. | G6, H1 |
| Git missing, checkout inaccessible, or recovery permissions denied | Report failure while retaining available recovery data. | H2, H3 |

## Remotes and authentication

| Scenario | Expected behavior | Coverage |
|---|---|---|
| Named remote, literal SSH/HTTPS URL, file URL or local path | Resolve the connection's actual destinations through Git. | G2, G3, transport smoke |
| Separate fetch/push URLs | Fetch both histories and publish only to the configured push destinations. | G3 |
| Multiple push URLs | Confirm every destination before acknowledging synchronization. | G3 |
| One mirror accepts and another rejects | Retain the queue; retry the same commit until all destinations confirm it. | G7 |
| Remote configuration changes after testing | Require connection verification before publication. | G3 |
| URL rewriting changes credential destination | Refuse to send saved credentials to a different endpoint. | A1, G8 |
| HTTPS system credential helper | Preserve Git's helper; authenticate first publication over TLS. | transport smoke |
| Explicit HTTPS username and password/token | Supply only to verified host, port and repository path. | A1 |
| Reconnect with new HTTPS credentials | Test a new credential reference while preserving the previous saved connection on failure/cancel. | H4 |
| System SSH configuration and agent | Leave system authentication intact. | G9, transport smoke |
| Connection-specific SSH key | Quote special-character paths and apply the key only to that subprocess. | G9 |
| Generated SSH key | Create unique private keys and matching public keys without replacing existing keys. | S1 |
| Encrypted SSH key and optional remembered passphrase | Validate OpenSSH compatibility and keep remembering scoped to the selected key. | S1, G9 |
| Authentication denied, branch protection or server-side rejection | Retain the local capture/commit and permit retry. | G7, GitHub setup tests |
| Slow/stalled helper, SSH process or Git hook | Cancellation/timeout terminates the process group and retains recovery data. | P1, H1 |

## Save and synchronization

| Scenario | Expected behavior | Coverage |
|---|---|---|
| Unrelated staged, unstaged and untracked files | Preserve their bytes and index state; omit them from the save commit. | G1, G2, G4 |
| Captured design paths contain incompatible local changes | Refuse to overwrite those paths and retain the queued save. | G2, H5 |
| CRLF checkout / automatic line-ending conversion | Preserve exact storage bytes with project-local attributes; reopen with matching hashes. | G11 |
| Repository ignores the cloud project | Explicitly stage owned save paths while preserving unrelated ignored files. | G11 |
| Custom project attributes | Preserve the existing file and require reconciliation before changing it. | G11, API3 |
| Higher-priority filter changes captured bytes | Keep the capture local; reject publication on initial attempt and retry. | G11 |
| Ordinary Git hooks, identity and signing configuration | Use standard `git commit`; preserve its checks and configuration. | G10 |
| No configured author identity | Supply the app identity for this commit only. | H6 |
| Save without document changes | Keep the current revision. | E1 |
| Retry after commit/push before acknowledgment | Recognize the captured revision and reuse the same commit. | G1, G7, H7, API1 |
| Download with no queued saves | Pull peer updates and update cached heads. | H5 |
| Remote branch is behind the local saved commit | Push the captured commit without including later unrelated commits. | G2, G7 |
| Git histories diverge | Pause fast-forward publication; preserve both histories and queued data. | G10 |
| Push race or peer writes the same design | Preserve the peer version and keep the captured version recoverable. | API1, C1, H8 |
| Different designs change in the same repository | Retain the base tree's other designs and assets. | G2, API1 |
| Unchanged imported meshes/images | Reuse content-addressed assets. | API1, format tests |
| New/unknown format or incompatible codec | Ask for upgrade before decoding or writing, including retries. | API2, H2, format tests |

## Offline operation, restart and conflicts

| Scenario | Expected behavior | Coverage |
|---|---|---|
| Save with no connectivity | Persist the snapshot first and leave it queued. | G7, H2, H7 |
| Restart during an upload or with queued saves | Restore local listings, editable data and pending revision identities. | H2, H7, browser recovery fixture |
| Cached design opened offline or checkout lost | Open the validated local snapshot. | H2, browser recovery fixture |
| Multiple repositories, one busy/broken/offline | Keep other queues and local saves usable independently. | H3 |
| Save while synchronization is already running | Persist immediately and wait for the connection's publisher. | H1 |
| Edits while an older capture uploads | Acknowledge only that capture; retain later edits as unsaved. | E1 |
| Two offline versions of one design | List competing versions rather than choose one silently. | H8 |
| Child saves after a conflict copy | Follow the confirmed copy across restarts without duplicate revisions. | H8, API1 |
| Keep local / Keep remote | Compare both observed heads and publish one atomic resolution with recovery history. | C1, desktop conflict smoke |
| Rename / keep both | Keep both live designs; check target occupancy. | H9, API1, desktop conflict smoke |
| Delete, Trash, history and revert | Queue/retry management as recoverable revisions. | H9, API1 |
| Interrupted resolution and later remote edits | Resume idempotently without replacing a newer original head. | C1, desktop conflict smoke |
| Interrupted receipt/cache update | Reconcile queued data and receipts while preserving newer cached heads. | H7, browser recovery fixture |

## Regression groups

The names below are searchable test names, grouped by source file. The matrix
records coverage, while each development run reports which subset was executed.

- **G1** — `io/src/cloud/git.rs`: `empty_checkout_publishes_first_design_without_committing_unrelated_staged_files`.
- **G2** — same file: `native_git_publishes_exact_paths_and_preserves_unrelated_work`, `failed_publication_keeps_the_durable_local_snapshot_and_is_connection_scoped`, `externally_changed_design_paths_are_preserved_before_any_save_writes`.
- **G3** — same file: `named_remotes_confirm_every_push_destination_and_pin_their_configuration`, `a_distinct_push_destination_receives_its_first_branch_and_fetch_stays_intact`.
- **G4** — same file: `linked_worktrees_publish_their_branch_and_preserve_their_separate_index`.
- **G5** — same file: `external_git_operations_and_wrong_branches_pause_before_writing`.
- **G6** — same file: `checkout_lock_prevents_overlapping_publication`.
- **G7** — same file: `rejected_initial_push_resumes_the_same_commit_from_the_durable_queue`.
- **G8** — same file: `guided_https_refuses_git_url_rewrites_before_network_access`.
- **G9** — same file: `custom_ssh_paths_are_quoted_and_system_auth_is_untouched`, `remembered_ssh_helper_is_process_and_identity_scoped`.
- **G10** — same file: `divergent_histories_and_commit_checks_preserve_local_work_for_retry`.
- **G11** — same file: `cloud_projects_keep_exact_bytes_across_line_endings_and_repository_ignore_rules`, `higher_priority_git_filters_cannot_publish_changed_capture_bytes_even_on_retry`.
- **H1** — `io/src/cloud/host/tests.rs`: `saving_during_another_synchronization_waits_and_publishes_the_durable_capture`, `cancelling_a_save_waiting_for_synchronization_retains_its_capture_for_retry`.
- **H2** — same file: `restart_lists_and_opens_latest_offline_capture_without_a_checkout_or_git`, `newer_or_corrupt_checkout_content_never_silently_opens_a_stale_cached_design`, `asynchronous_git_operations_retain_the_callers_allowed_folders`.
- **H3** — same file: `a_busy_or_damaged_connection_cannot_block_other_queues_or_local_saves`.
- **H4** — `ui-egui/src/cloud_settings/git/tests.rs`: `https_credentials_are_editable_and_replacements_wait_for_verification_and_apply`.
- **H5** — `io/src/cloud/host/tests.rs`: `synchronization_pulls_peer_updates_with_an_empty_queue_and_preserves_unrelated_work`, `synchronization_pulls_outdated_caches_and_preserves_unrelated_git_work`, `changed_checkout_is_not_marked_synced_and_cache_failures_never_open_an_older_head`.
- **H6** — same file: `guided_checkout_setup_retries_and_pushes_without_git_identity_configuration`.
- **H7** — same file: `save_jobs_report_their_own_revision_and_restart_sync_is_idempotent`, `confirmed_catalog_survives_lost_checkout_and_old_receipts_cannot_replace_newer_heads`.
- **H8** — same file: `confirmed_conflict_copies_reopen_at_their_actual_destinations_after_restart`, `offline_children_follow_a_confirmed_conflict_copy_across_restart_without_duplicate_revisions`, `competing_offline_captures_remain_available_instead_of_opening_an_arbitrary_tip`.
- **H9** — same file: `management_renames_trashes_restores_and_resumes_without_touching_other_work`, `queued_management_can_be_discarded_without_losing_the_saved_design`.
- **A1** — `io/src/cloud/auth.rs`: both credential protocol tests.
- **S1** — `io/src/cloud/ssh/tests.rs`: generated/encrypted key tests.
- **P1** — `io/src/cloud/process.rs`: bounded output, pipe draining, cancellation and deadline tests.
- **API1** — `cloud/src/github/tests.rs`: atomic management, asset reuse, competing saves, offline children and ref-race tests.
- **API2** — same file: future formats, symlinks, corrupt existing objects and cached-opening tests.
- **API3** — same file: `custom_project_attributes_are_preserved_before_any_remote_write`.
- **C1** — `cloud/src/publication.rs`: `conflict_resolution_checks_both_heads_is_atomic_and_retries_without_replacing_newer_edits` and ordinary publication tests.

- **E1** — `engine/src/cloud/tests.rs`: `unchanged_cloud_save_keeps_the_revision_and_submits_no_work`, `cloud_capture_becomes_the_saved_baseline_without_losing_later_edits`.

Paths above are relative to `crates/`.
Browser recovery and authenticated interoperability commands are documented in
[the developer guide](cloud-saves.md).

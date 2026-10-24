# Cloud Saves

Cloud Saves keeps designs in Git repositories. Each repository can contain project
folders, with parts, assemblies and related documents inside each project.
You can connect more than one repository.

## Connect GitHub

1. On **Start**, find **Cloud Saves** and choose **Connect GitHub**.
2. Choose **Create a private repository**. GitHub opens with the name
   `solidcraft-designs` and private visibility selected. Check these choices and
   create the repository. You can also use a repository you already own.
3. Back in SolidCraft, enter its `owner/repository` name, or paste its GitHub URL.
4. Choose **Create access token**. GitHub opens with the token name, description,
   repository owner, **Contents: Read and write**, and **No expiration** prefilled.
   Choose **Only select repositories**, select this repository, check the permission,
   then choose **Generate token**. GitHub requires you to select the repository yourself.
   An organization policy may require an expiration or approval.
5. Copy the generated token, paste it into SolidCraft, and choose **Connect GitHub**. SolidCraft checks
   access and write permission, initializes an empty repository, and remembers the
   connection. A desktop connection also creates a local checkout automatically.

An organization may require approval before its token works. A read-only token,
an expired token, a renamed repository or an unavailable keychain produces an error;
correct the problem and retry. Reconnecting the same repository updates that
connection, leaving other connections and their default selection intact.

This guided setup works on desktop and in the browser. Create the token once;
SolidCraft remembers it for future saves, including after a browser restart.

Desktop stores tokens in the operating system's credential store. The browser
keeps them in a separate local credential database. Tokens are never included in
designs or shared repository metadata. Treat a token like a password, and use a
trusted browser profile and device. Desktop uses the same token for Git over HTTPS;
you do not need an SSH key or a GitHub password.

## Save and synchronize

1. On **Start → Cloud Saves**, choose **Save to Cloud Project**. For a new design,
   normal **Save** opens the same picker when a default cloud repository is connected.
2. Choose a repository, enter a project folder (or choose an existing one), and name
   the design. For example, project `Robot` and name `bracket` create
   `Robot/parts/bracket` inside the repository. Parts and assemblies use the same
   design representation; you can also store ordinary documents in the project.
3. Choose **Save**. Future saves use this destination even if you change the default
   repository. Native **Save As** creates a `.solidcraft` file and detaches that
   document from its cloud destination.

On **Start**, use the repository selector to browse connected repositories and
click a design to open it. Other open designs and unsaved edits are preserved.
**Refresh** reloads the list; **Synchronize Repository** retries pending saves for
the selected repository. A failed refresh keeps the last displayed list; availability
offline depends on the designs retained by local storage. Connect another repository
with **Connect GitHub**; each connection has independent credentials and queues.

The picker refuses names already shown in that project. If another device creates
the same name after listing, synchronization preserves a conflict copy. Changing
the active design while the picker is open cancels the choice so it cannot save a
different tab by accident. Thumbnail previews are under development.

Saving first records the captured design on this device, then synchronizes it.
**Saved on this device** means it is recoverable here and still needs publication.
**Syncing** means publication is running. **Synced** means the remote accepted it.
Edits made while an upload runs remain unsaved until you save again.

Without connectivity, saves stay in the local queue. Keep the same browser profile:
reopening SolidCraft resumes pending saves, and queued or previously confirmed
designs can be opened offline. The browser requests persistent storage; clearing
site data or deleting the browser profile also removes local recovery data and
credentials. Storage errors are reported instead of claiming the save succeeded.
Desktop retains its recovery queue separately from the checkout.

To retry immediately, run `cloud.sync {"connection":"<connection id>"}`.
Cancelling synchronization keeps queued designs. When a conflicting design edit
is found, SolidCraft preserves a conflict copy rather than replacing either edit.
Newer storage formats ask you to upgrade before opening or saving.

## Manage connections

Open **Cloud Saves Settings** from Start, or choose **Settings → Cloud Saves**.
Select a repository to rename it, make it the default for new cloud saves, test
its connection, synchronize it, or reconnect with a replacement token.

Connection changes take effect when you choose **Apply** or **OK** and local
storage confirms them. A failed Apply keeps the page open so you can retry.
**Cancel** discards changes made since the last successful Apply. Connecting or
reconnecting from this page verifies the repository first, then waits for Apply.

Choose **Disconnect**, then Apply to remove a connection from the app. Its queued
saves remain on this device; reconnecting the same repository restores access to
them. Existing designs keep their destinations when you change the default.
**Restore Defaults** resets synchronization preferences and preserves connections.

**Synchronize after saving** publishes each locally recorded save in the background.
**Retry pending saves automatically** retries after startup and at regular intervals.
You can also synchronize a selected repository immediately or cancel an active
operation. Cancelling an upload keeps its durable local save.

## Reconnect and desktop Git

Use **Connect GitHub** again with the same repository and a replacement token.
Leave the token field blank to reuse the saved token after a temporary network
failure. A failed replacement does not replace a working connection's credential.

An app-managed checkout is an ordinary Git checkout that a desktop client can use.
SolidCraft commits the files belonging to its save and preserves unrelated staged
and unstaged changes. It waits for external merges or rebases to finish.
Git configuration and signing remain in effect. If no author identity is available,
the app uses `SolidCraft <cloud@solidcraft.invalid>` for that save only.

Advanced connection commands also support existing checkouts and SSH, HTTPS,
`file://` or local remotes. System SSH configuration and an explicitly selected
key work through Git. The generated-key workflow is under development. Do not put tokens into remote URLs.

See GitHub's [token guide](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens)
for token expiration, organization approval and revocation.

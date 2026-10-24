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

This feature is under development. The connection wizard is available; the
repository browser and destination picker are still being built. For now, the
command palette's JSON form can save a design using the registered command:

```text
cloud.save {"destination":{"connection":"<connection id>","project":"Robot","file":"parts/bracket"}}
```

`cloud.settings {}` lists your connection identifiers. Once a design has a cloud
destination, normal **Save** uses it. Native **Save As** creates a `.solidcraft`
file and detaches that document from its cloud destination.

Saving first records the captured design on this device, then synchronizes it.
**Saved locally** means it is recoverable here and still needs publication.
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
key work through Git. The dedicated settings editor and generated-key workflow
are still being developed. Do not put tokens into remote URLs.

See GitHub's [token guide](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens)
for token expiration, organization approval and revocation.

# Keeping designs on a network share (Nextcloud, ownCloud, a NAS, WebDAV)

SolidCraft opens and saves `.sc` designs in any folder. To keep designs on a server, mount the
share with your operating system and use it like a local folder: **File → Open** and **File → Save**
work on it as they do anywhere else. SolidCraft never asks for a server address or password, so
your credentials stay with the operating system.

You need the share's address (for Nextcloud or ownCloud, the WebDAV address on your account's
settings page) and your user name and password or app password.

## Windows

- **Map network drive.** In File Explorer choose **This PC → Map network drive**, pick a drive
  letter, and enter the WebDAV address (`https://…`) or a `\\server\share` path. The share then
  appears as a drive in SolidCraft's file dialogs.
- **Sync client.** Install the Nextcloud or ownCloud desktop sync client and open designs from its
  synced folder.

## macOS

- **Connect to Server.** In Finder choose **Go → Connect to Server**, enter the address
  (`https://…` for WebDAV, `smb://…` or `nfs://…` for a NAS) and sign in. The share is mounted
  under `/Volumes`.
- **Sync client.** Install the Nextcloud or ownCloud desktop sync client and open designs from its
  synced folder.

## Linux

- **File manager.** In GNOME Files or KDE Dolphin, open **Other Locations** (or the network
  places) and connect to `davs://host/path`. The share appears as a folder you can pick in
  SolidCraft's file dialogs.
- **`davfs2`.** Install `davfs2`, then add a line to `/etc/fstab`:

  ```
  https://cloud.example.com/remote.php/dav/files/you/  /mnt/designs  davfs  user,noauto  0  0
  ```

  Put your credentials in `~/.davfs2/secrets`, readable only by you, then run `mount /mnt/designs`.
- **Sync client.** Install the Nextcloud or ownCloud desktop sync client and open designs from its
  synced folder.

## A NAS

SMB (Windows file sharing) and NFS shares from a NAS are mounted the same way: map the drive on
Windows, use Connect to Server on macOS, or mount them from your file manager or `/etc/fstab` on
Linux. Once mounted, SolidCraft sees an ordinary folder.

## Sync client or live mount

A sync client keeps a local copy of the folder, so you can open and save designs offline and the
client uploads your changes when you are back online. A live mount has no local copy and needs the
network while you open and save.

## In the browser

The web app reaches a mounted share through the browser's file picker, so what works depends on the
browser:

- **Chrome, Edge and other Chromium browsers** let the page open a design from any folder you pick,
  the mounted share included, and save it back to the same file.
- **Firefox and Safari** have no such picker: the web app keeps designs in the browser's own storage,
  so you open a design from the share with **File → Open** (an upload) and save it by downloading it
  into the share's folder.

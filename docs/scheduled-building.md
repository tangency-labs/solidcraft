# Automated Build & Deployment Pipeline

SolidCraft includes automated build and deployment scripts for Windows (`update-solidcraft.ps1`) and Linux (`update-solidcraft.sh`).

The scripts check the selected Git branch for updates, build native GUI and CLI executables, optionally build the WebAssembly target, and deploy successful binaries. Builds run in an isolated Git worktree so the user's original checkout, current branch, and uncommitted changes remain untouched.

## Key Features

- **Branch-aware builds:** By default, builds the upstream branch associated with the current checkout. An optional branch argument allows building another branch, such as `main`, without switching the original checkout.
- **Isolated build environment:** Uses a temporary detached Git worktree at the exact fetched commit. The original working tree and uncommitted changes are not modified.
- **Incremental build detection:** Tracks the commit and build configuration to avoid unnecessary builds when the selected revision has already been built successfully.
- **Selective target builds:** GUI, CLI, and WebAssembly targets can be enabled or disabled independently.
- **Deployment backups:** Existing native executables are backed up with timestamps before replacement.
- **Profile-aware cleanup:** Force builds clean generated contents of the selected Cargo profile while preserving the backup directory.
- **Shared Cargo target directory:** Reuses the repository's `target` directory to benefit from existing compiled dependencies.
- **Automated logging:** Saves timestamped logs under `scripts/build_logs/`.
- **Headless execution:** Supports scheduled execution through Windows Task Scheduler or Linux `cron`.

## Build Architecture

### Isolated Git Worktree

Rather than switching branches or pulling changes into the user's active checkout, the scripts:

1. Fetch the selected branch from its remote.
2. Resolve the exact commit to build.
3. Create a temporary detached Git worktree at that commit.
4. Build from the temporary worktree.
5. Deploy successfully built artifacts.
6. Remove the temporary worktree.

This approach protects local modifications, including newly created files, from branch changes and build operations.

The original checkout remains on its original branch throughout the process.

### Build Targets

| Target | Cargo Package | Output | Supported Host |
|---|---|---|---|
| GUI | `solidcraft` | Native executable (`solidcraft` / `solidcraft.exe`) | Windows / Linux |
| CLI | `solidcraft-cli` | Native executable (`solidcraft-cli` / `solidcraft-cli.exe`) | Windows / Linux |
| Web | `solidcraft-web` | WebAssembly | Windows / Linux |

### Deployment Locations

Native executables are deployed to the repository root:

| Platform | GUI | CLI |
|---|---|---|
| Linux | `solidcraft` | `solidcraft-cli` |
| Windows | `solidcraft.exe` | `solidcraft-cli.exe` |

Before replacing an existing executable, the script creates a timestamped backup under:

- Linux: `target/<profile>/backup/`
- Windows: `target\<profile>\backup\`

The profile is either `debug` or `release`.

The WebAssembly target uses a separate installation directory configured through `WEB_INSTALL_DIR`. It does not use the native executable deployment locations.

### Build Cleanup and Force Builds

The scripts maintain build-state information in:

- Linux: `target/.solidcraft-build-state`
- Windows: `target\.solidcraft-build-state`

The state records the selected revision and relevant build configuration. If the selected commit and configuration have already been built successfully, the script skips the build.

Use the force-build option to request a clean rebuild.

Force cleanup removes generated contents within the selected Cargo profile directory while preserving its `backup` subdirectory. It does not delete the entire `target` directory.

---

## Linux Setup (`update-solidcraft.sh`)

### Configuration

The script resolves the repository root automatically using Git. It uses the Cargo environment of the executing user.

The WebAssembly deployment directory can be configured using:

```bash
export WEB_INSTALL_DIR="/var/www/html/solidcraft/"
```

The default is:

```text
/var/www/html/solidcraft/
```

Native executable deployment does not require a separate installation directory.

### Usage

Run from the repository root:

```bash
./scripts/update-solidcraft.sh
```

The script can also be run from another working directory.

### Command-Line Options

| Option | Description |
|---|---|
| `-f`, `--force`, `--force-build` | Force a clean rebuild |
| `-s`, `--skip-tests` | Skip workspace tests |
| `-j`, `--jobs N` | Set the number of parallel build jobs |
| `-d`, `--debug` | Build using the debug profile |
| `-r`, `--release` | Build using the release profile |
| `-w`, `--web` | Enable the WebAssembly target |
| `--BuildGui <bool>` | Enable/disable GUI build (default: true) |
| `--BuildCli <bool>` | Enable/disable CLI build (default: true) |
| `-b`, `--branch BRANCH` | Build a specified branch instead of the current branch's upstream |
| `-h`, `--help` | Display help |

### Examples

Build only the CLI in the current branch's configured upstream using the default settings:

```bash
./scripts/update-solidcraft.sh --BuildGui false --BuildCli true
```

Build the latest `main` branch using the release profile, skipping tests:

```bash
./scripts/update-solidcraft.sh -s -r -b main
```

Force a clean release rebuild:

```bash
./scripts/update-solidcraft.sh -f -r
```

Build only the CLI:

```bash
./scripts/update-solidcraft.sh --BuildGui false
```

Enable the WebAssembly target:

```bash
./scripts/update-solidcraft.sh -w
```

### Web Deployment and Permissions

Deploying to a web root requires write permissions to the destination directory.

For scheduled execution as a standard user running via `cron`, the safest approach is to configure the web deployment path inside a directory owned entirely by your build user, bypassing system-wide protected folders completely:

1. Create a web deployment directory within your user home path or in the webserver path:

    mkdir -p "$HOME/public_html/solidcraft"
    - or
    mkdir -p "/var/www/html/solidcraft"
    - or
    mkdir C:\inetpub\wwwroot\solidcraft

2. Configure your web server (such as Nginx or Apache) to serve static assets and WebAssembly files directly from your user directory, eliminating the need for any privileged file copying.

Alternatively, if your workflow requires placing files into a shared web root, ensure the target directory is pre-configured and owned by your user account ahead of time by your system administrator, so your automated script can write to it directly without requiring privilege escalation.

### Setting Up a Cron Job

Open the user's crontab:

```bash
crontab -e
```

For example, to check for updates hourly:

```cron
0 * * * * /bin/bash "$HOME/solidcraft/scripts/update-solidcraft.sh" >/dev/null 2>&1
```

The script checks for new commits and skips the build when the selected revision and configuration have already been built.

If using a system-wide crontab, specify the executing user and load the Cargo environment:

```cron
0 * * * * username /bin/bash -c 'source "$HOME/.cargo/env" && "$HOME/solidcraft/scripts/update-solidcraft.sh"' >/dev/null 2>&1
```

---

## Windows Setup (`update-solidcraft.ps1`)

### Configuration

The script automatically resolves the repository root and uses the current user's Cargo environment.

The WebAssembly deployment directory can be configured through the `WEB_INSTALL_DIR` environment variable.

The default is:

```text
C:\inetpub\wwwroot\solidcraft
```

Native GUI and CLI executables are deployed to the repository root.

### Usage

Run from PowerShell:

```powershell
.\scripts\update-solidcraft.ps1
```

### Parameters

| Parameter | Description |
|---|---|
| `-ForceBuild` | Force a clean rebuild |
| `-SkipTests` | Skip workspace tests |
| `-Jobs N` | Set the number of parallel build jobs |
| `-DebugMode` | Build using the debug profile |
| `-ReleaseMode` | Build using the release profile |
| `-BuildWeb` | Enable the WebAssembly target |
| `-BuildGui <bool>` | Enable/disable the GUI target (default: `$true`) |
| `-BuildCli <bool>` | Enable/disable the CLI target (default: `$true`) |
| `-Branch BRANCH` | Build a specified branch instead of the current branch's upstream |

### Examples

Build the current branch's configured upstream:

```powershell
.\scripts\update-solidcraft.ps1
```

Build `main` using the release profile, skipping tests:

```powershell
.\scripts\update-solidcraft.ps1 -Branch main -ReleaseMode -SkipTests
```

Force a clean release rebuild:

```powershell
.\scripts\update-solidcraft.ps1 -ForceBuild -ReleaseMode
```

Build only the CLI:

```powershell
.\scripts\update-solidcraft.ps1 -BuildGui $false
```

Enable building the WebAssembly target:

```powershell
.\scripts\update-solidcraft.ps1 -BuildWeb
```

### Setting Up Windows Task Scheduler

To execute the build pipeline automatically:

1. Open **Task Scheduler** and select **Create Basic Task**.
2. Set the desired trigger (for example, Daily or Hourly).
3. Select **Start a program** as the action.
4. Configure the following:

   - **Program/script:** `powershell.exe`
   - **Add arguments:**
     ```text
     -ExecutionPolicy Bypass -File "C:\solidcraft\scripts\update-solidcraft.ps1"
     ```
   - **Start in:** `C:\solidcraft`

The scheduled task should run under an account with access to the repository, Git, Rust/Cargo, and the configured deployment directory.

---

## Persistent Build Logs

Both scripts automatically create timestamped log files in:

```text
scripts/build_logs/
```

Example filenames:

- Linux: `update_YYYYMMDD_HHMMSS_PID.log`
- Windows: `update_YYYYMMDD_HHMMSS_PID.log`

Logs contain Git fetch information, selected revisions, build steps, compiler output, deployment results, and errors.

### Identifying Errors

Major operations are identified with numbered step markers such as:

```text
[STEP 1] Fetching updates...
[STEP 2] Preparing detached build worktree...
[STEP 3] Running workspace tests...
[STEP 4] Building native GUI binary...
```

Fatal failures and compilation errors are reported in the logs with prominent error markers.

Review the log corresponding to the execution when troubleshooting a failed build or deployment.

## Important Notes

- The scripts do not switch branches, stash changes, reset the checkout, or modify uncommitted files in the original working tree.
- Building a different branch does not change the branch currently checked out in the repository.
- The Cargo `target` directory is shared with the original repository to reuse compiled artifacts.
- Avoid running multiple instances of the scripts concurrently against the same repository and Cargo target directory.
- Native binaries are backed up before deployment. Backups are retained under the selected profile's `backup` directory.
- The `WEB_INSTALL_DIR` setting applies only to WebAssembly deployment, not native executables.

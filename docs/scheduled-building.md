# Automated Build & Deployment Pipeline

SolidCraft includes automated build and deployment scripts for both Windows (`update-solidcraft.ps1`) and Linux (`update-solidcraft.sh`) environments. These scripts pull the latest repository changes, run workspace regression tests, compile selected release targets, and deploy binaries atomically.

## Key Features

* **Atomic Deployment:** Uses temporary staging files (`_new` binaries, moving active binaries to `_old`) to ensure live binaries are replaced cleanly without file-lock collisions.
* **Isolated Target Fault Tolerance:** Build steps for GUI, CLI, and Web targets are isolated. If a single target compilation fails, successfully built targets are still safely deployed.
* **Fail-Fast Safety:** Fatal blockers (such as `git pull` failures or test suite regressions) immediately halt the pipeline before any binaries are modified.
* **Automated Logging:** Saves timestamped build logs with prominent ASCII error markers to `build_logs/` for easy auditing.
* **Headless Execution:** Fully compatible with scheduled tasks (Windows Task Scheduler or Linux `cron`).

## Script Architecture & Build Targets

| Target Component | Package Name | Target Output | Host OS | 
| ----- | ----- | ----- | ----- | 
| **GUI** | `solidcraft` | Native Executable (`.exe` / ELF) | Windows / Linux | 
| **CLI** | `solidcraft-cli` | Native Executable (`.exe` / ELF) | Windows / Linux | 
| **Web** | `solidcraft-web` | WebAssembly (`wasm32-unknown-unknown`) | Linux / Windows | 

---

## Linux Setup (`update-solidcraft.sh`)

### Configuration

Open `update-solidcraft.sh` and set your desired target toggles and deployment paths at the top of the file:

```bash
# Configuration Toggles (Modify defaults to true or false to build component)
BUILD_GUI=true
BUILD_CLI=true
BUILD_WEB=false

# Dynamic Environment & Paths
export HOME="$(getent passwd $USER | cut -d: -f6)"
source "$HOME/.cargo/env"
REPO_DIR="$HOME/solidcraft"
WEB_DIR="/var/www/html/solidcraft"
```

> **Permissions Note for Web Deployment (`BUILD_WEB=true`):**  
> Deploying to web roots like `/var/www/html/solidcraft` requires write permissions. If running via a standard user `cron` job, choose one of the following approaches to prevent `sudo` from hanging:
>
> 1. **Grant Directory Ownership (Recommended):** Change ownership of the target directory to your build user:
>    ```bash
>    sudo mkdir -p /var/www/html/solidcraft
>    sudo chown -R $USER:$USER /var/www/html/solidcraft
>    ```
>    *(If using this method, you can safely remove `sudo` from the copy commands in `update-solidcraft.sh`.)*
>
> 2. **Passwordless Sudo (visudo):** Allow your build user to execute copy commands to the web root without a password prompt by adding an entry in `/etc/sudoers.d/solidcraft-deploy`:
>    ```text
>    username ALL=(ALL) NOPASSWD: /usr/bin/mkdir -p /var/www/html/*, /usr/bin/cp -f *, /usr/bin/chown *
>    ```

### Usage

Make the script executable and run manually:

```bash
chmod +x scripts/update-solidcraft.sh
./scripts/update-solidcraft.sh
```

### Setting up a Cron Job

To automatically check for updates and rebuild on a schedule (e.g., every hour):

```bash
crontab -e
```

Add the following entry (using `$HOME` or dynamic pathing so it automatically adapts to the user or root environment running the cron job):

```cron
0 * * * * /bin/bash "$HOME/solidcraft/scripts/update-solidcraft.sh" >/dev/null 2>&1
```

If scheduling the job within a system-wide crontab (`/etc/crontab`), specify the executing user dynamically:

```cron
0 * * * * username /bin/bash -c 'source "$HOME/.cargo/env" && "$HOME/solidcraft/scripts/update-solidcraft.sh"' >/dev/null 2>&1
```

---

## Windows Setup (`update-solidcraft.ps1`)

### Configuration Parameters

The PowerShell script uses a parameter block at the top of the file to manage build target toggles:

```powershell
param(
    [bool]$BuildGui = $true,
    [bool]$BuildCli = $true,
    [bool]$BuildWeb = $false
)
```

### Usage

Run directly from PowerShell:

```powershell
# Run with defaults (GUI = True, CLI = True, Web = False)
.\scripts\update-solidcraft.ps1

# Override flags on execution (e.g., enable Web target, disable GUI target)
.\scripts\update-solidcraft.ps1 -BuildWeb $true -BuildGui $false
```

### Setting up Windows Task Scheduler

To execute the build pipeline automatically:

1. Open **Task Scheduler** and click **Create Basic Task**.
2. Set your desired trigger (e.g., Daily or Hourly).
3. Select Action: **Start a program**
   * **Program/script:** `powershell.exe`
   * **Add arguments:** `-ExecutionPolicy Bypass -File "C:\solidcraft\scripts\update-solidcraft.ps1"`
   * **Start in:** `C:\solidcraft`

---

## Persistent Build Logs

All console output, compilation errors, and test suite results are recorded automatically in timestamped log files located in:

* **Linux:** `$REPO_DIR/build_logs/build_YYYYMMDD_HHMMSS.log`
* **Windows:** `$RepoDir\build_logs\build_YYYYMMDD_HHMMSS.log`

### Identifying Log Errors

Major steps (`[STEP 1]` through `[STEP 7]`) are formatted with clear horizontal banners. Any fatal failures or compilation errors trigger high-visibility ASCII borders within the log file:

```text
####################################################
 [FATAL ERROR]: CLI compilation failed!
####################################################
```
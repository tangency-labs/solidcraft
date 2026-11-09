#!/usr/bin/env bash
set -Eeuo pipefail

# WEB_INSTALL_DIR is used only as the WebAssembly deployment directory.
WEB_INSTALL_DIR="${WEB_INSTALL_DIR:-/var/www/html/solidcraft/}"
BUILD_GUI="${BUILD_GUI:-true}"
BUILD_CLI="${BUILD_CLI:-true}"
BUILD_WEB="${BUILD_WEB:-false}"
FORCE_BUILD=false
SKIP_TESTS=false
BRANCH_OVERRIDE=""
JOBS=0
PROFILE="release"
CARGO_PROFILE_FLAG=(--release)

show_usage() {
    cat << EOF
SolidCraft Linux Build & Update Script

Usage: $0 [options]

Options:
    -f, --force, --force-build     Force a clean rebuild even if Git commits match.
    -s, --skip-tests               Skip running workspace tests before building.
    -b, --branch <NAME>            Build a remote branch (default: current branch upstream).
    -j, --jobs <N>                 Set number of parallel compilation jobs (default: all cores).
    -d, --debug                    Build using debug profile.
    -r, --release                  Build using release profile (default).
    -w, --web                      Build and deploy WebAssembly target.
    --no-gui                       Disable native GUI build.
    --no-cli                       Disable CLI build.
    -h, --help                     Display this help message.

Native binaries are deployed to the repository root; timestamped backups are stored under target/<profile>/backup/.
The WebAssembly target is deployed to WEB_INSTALL_DIR (default: /var/www/html/solidcraft/).
EOF
}

while (($#)); do
    case "$1" in
        -f|--force|--force-build) FORCE_BUILD=true; shift ;;
        -s|--skip-tests) SKIP_TESTS=true; shift ;;
        -b|--branch)
            [[ $# -ge 2 ]] || { echo "Error: --branch requires a branch name." >&2; exit 2; }
            BRANCH_OVERRIDE="$2"
            [[ -n "$BRANCH_OVERRIDE" ]] || { echo "Error: --branch cannot be empty." >&2; exit 2; }
            shift 2 ;;
    -j|--jobs)
            [[ $# -ge 2 ]] || { echo "Error: --jobs requires a value." >&2; exit 2; }
      JOBS="$2"
            [[ "$JOBS" =~ ^[1-9][0-9]*$ ]] || { echo "Error: --jobs must be a positive integer." >&2; exit 2; }
            shift 2 ;;
        -d|--debug) PROFILE="debug"; CARGO_PROFILE_FLAG=(); shift ;;
        -r|--release) PROFILE="release"; CARGO_PROFILE_FLAG=(--release); shift ;;
        -w|--web) BUILD_WEB=true; shift ;;
        --no-gui) BUILD_GUI=false; shift ;;
        --no-cli) BUILD_CLI=false; shift ;;
        -h|--help) show_usage; exit 0 ;;
        *) echo "Error: Unknown option '$1'" >&2; echo "Run '$0 --help' for usage information." >&2; exit 2 ;;
  esac
done

for flag in BUILD_GUI BUILD_CLI BUILD_WEB; do
    value="${!flag}"
    [[ "$value" == true || "$value" == false ]] || { echo "Error: $flag must be true or false (got '$value')." >&2; exit 2; }
done

# Scheduled tasks may start in an unrelated working directory.
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
LOG_DIR="$SCRIPT_DIR/build_logs"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/update_$(date +%Y%m%d_%H%M%S)_$$.log"
exec > >(tee -a "$LOG_FILE") 2>&1

fail() { echo "ERROR: $*" >&2; exit 1; }

# Run Cargo and Git from the repository root, not the scripts/ directory.
REPO_ROOT="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null)" || fail "Unable to locate the Git repository containing this script."
cd "$REPO_ROOT"

deploy_binary() {
    local source="$1" destination="$2" label="$3" backup_dir="$4"
    local backup_file staged timestamp
    mkdir -p "$backup_dir"
    staged="${destination}.new.$$"
    timestamp="$(date +%Y%m%d_%H%M%S)_$$"
    backup_file="$backup_dir/$(basename -- "$destination").$timestamp"

    cp -p -- "$source" "$staged" || { rm -f -- "$staged"; fail "$label: unable to stage new binary."; }
    if [[ -f "$destination" ]]; then
        cp -p -- "$destination" "$backup_file" || { rm -f -- "$staged"; fail "$label: unable to back up existing binary."; }
        echo "$label: previous binary backed up to $backup_file"
    fi
    if ! mv -f -- "$staged" "$destination"; then
        rm -f -- "$staged"
        fail "$label: replacement failed; existing binary was left in place and backup retained if created."
fi
    echo "$label deployment successful: $destination"
}

# Clear only target/<profile> generated contents, preserving target/ and backup/.
clean_build_artifacts_preserving_backups() {
    local target_dir="$REPO_ROOT/target/$PROFILE"
    local backup_dir="$target_dir/backup"
    mkdir -p "$backup_dir"
    # Keep target/ and target/<profile>/ in place. Remove only the selected
    # profile's generated contents, preserving backup/ and avoiding Cargo's
    # broader target-directory cleanup (which can fail if another process has
    # the target directory itself open).
    find "$target_dir" -mindepth 1 -maxdepth 1 ! -name backup -exec rm -rf -- {} +
    echo "Cleaned generated contents of $target_dir; preserved $backup_dir."
}

echo "=================================================="
echo "Starting SolidCraft update: $(date)"
echo "Log file: $LOG_FILE"
echo "Selected build profile: $PROFILE"
if (( JOBS > 0 )); then JOBS_ARG=(--jobs "$JOBS"); else JOBS_ARG=(); fi

git rev-parse --show-toplevel >/dev/null 2>&1 || fail "Not running inside a Git repository."

# Build from a detached worktree so local changes in the operator's checkout
# are never stashed, reset, overwritten, or included accidentally.
# By default, build the upstream configured for the currently checked-out
# branch. --branch overrides this with a remote-tracking branch (e.g. main or
# origin/main). The original checkout is never switched, reset, or stashed.
CURRENT_BRANCH="$(git symbolic-ref --quiet --short HEAD 2>/dev/null || true)"
if [[ -n "$BRANCH_OVERRIDE" ]]; then
    if [[ "$BRANCH_OVERRIDE" == refs/remotes/* ]]; then
        SOURCE_REF="$BRANCH_OVERRIDE"
    elif [[ "$BRANCH_OVERRIDE" == */* ]]; then
        BRANCH_REMOTE="${BRANCH_OVERRIDE%%/*}"
        if git remote get-url "$BRANCH_REMOTE" >/dev/null 2>&1; then
            SOURCE_REF="refs/remotes/$BRANCH_OVERRIDE"
        else
            SOURCE_REF="refs/remotes/origin/$BRANCH_OVERRIDE"
        fi
    else
        SOURCE_REF="refs/remotes/origin/$BRANCH_OVERRIDE"
    fi
else
    [[ -n "$CURRENT_BRANCH" ]] || fail "Detached HEAD; specify --branch <name> to select a remote branch."
    SOURCE_REF="$(git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null)" || fail "Branch '$CURRENT_BRANCH' has no configured upstream; specify --branch <name>."
    SOURCE_REF="refs/remotes/$SOURCE_REF"
fi

echo "[STEP 1] Fetching updates for $SOURCE_REF..."
git -c maintenance.auto=false -c fetch.writeCommitGraph=false fetch --all --prune
SOURCE_COMMIT="$(git rev-parse "$SOURCE_REF^{commit}")" || fail "Unable to resolve selected remote branch '$SOURCE_REF'."
SOURCE_LABEL="${SOURCE_REF#refs/remotes/}"
STATE_KEY="$(printf '%s' "$SOURCE_REF|$PROFILE" | sha256sum | awk '{print $1}')"
STATE_DIR="$REPO_ROOT/target/.solidcraft-build-state"
STATE_FILE="$STATE_DIR/$STATE_KEY"
mkdir -p "$STATE_DIR"
BUILD_STATE="$SOURCE_REF|$SOURCE_COMMIT|$PROFILE|$BUILD_GUI|$BUILD_CLI|$BUILD_WEB|$SKIP_TESTS|$WEB_INSTALL_DIR"
LAST_BUILD_STATE=""
[[ -f "$STATE_FILE" ]] && LAST_BUILD_STATE="$(cat "$STATE_FILE")"

if [[ "$LAST_BUILD_STATE" == "$BUILD_STATE" && "$FORCE_BUILD" == false ]]; then
    echo "$SOURCE_LABEL ($SOURCE_COMMIT) is already built with this configuration. No build needed."
    echo "Use --force-build (-f) to force a clean rebuild."
    exit 0
fi
if [[ "$FORCE_BUILD" == true ]]; then
    echo "Force build requested: a clean rebuild of $SOURCE_LABEL will be performed."
else
    echo "New $SOURCE_LABEL revision/configuration detected: $SOURCE_COMMIT"
fi

BUILD_WORKTREE_PARENT="$(mktemp -d "${TMPDIR:-/tmp}/solidcraft-build.XXXXXX")"
BUILD_WORKTREE="$BUILD_WORKTREE_PARENT/checkout"
cleanup_build_worktree() {
    cd "$REPO_ROOT" 2>/dev/null || true
    if [[ -n "${BUILD_WORKTREE:-}" && -d "$BUILD_WORKTREE" ]]; then
        git -C "$REPO_ROOT" worktree remove --force "$BUILD_WORKTREE" >/dev/null 2>&1 || echo "WARNING: Could not remove temporary build worktree: $BUILD_WORKTREE" >&2
    fi
    if [[ -n "${BUILD_WORKTREE_PARENT:-}" && -d "$BUILD_WORKTREE_PARENT" ]]; then
        rmdir "$BUILD_WORKTREE_PARENT" 2>/dev/null || true
fi
}
trap cleanup_build_worktree EXIT

echo "[STEP 2] Preparing detached build worktree at $SOURCE_LABEL..."
git worktree add --detach "$BUILD_WORKTREE" "$SOURCE_COMMIT"
cd "$BUILD_WORKTREE"
export CARGO_TARGET_DIR="$REPO_ROOT/target"

TARGET_DIR="$REPO_ROOT/target/$PROFILE"
mkdir -p "$TARGET_DIR/backup"

if [[ "$FORCE_BUILD" == true ]]; then
    echo "[STEP 2A] Cleaning Cargo artifacts while preserving deployed-binary backups..."
    clean_build_artifacts_preserving_backups
fi

if [[ "$SKIP_TESTS" == true ]]; then
    echo "[STEP 3] Skipping workspace tests (--skip-tests active)."
else
    echo "[STEP 3] Running workspace tests ($PROFILE mode)..."
    cargo test --workspace "${CARGO_PROFILE_FLAG[@]}" "${JOBS_ARG[@]}"
fi

TARGET_DIR="$REPO_ROOT/target/$PROFILE"
mkdir -p "$TARGET_DIR/backup"

if [[ "$BUILD_GUI" == true ]]; then
    echo "[STEP 4] Building native GUI binary ($PROFILE)..."
    cargo build "${CARGO_PROFILE_FLAG[@]}" --bin solidcraft "${JOBS_ARG[@]}"
    SOURCE_GUI="$TARGET_DIR/solidcraft"
    [[ -f "$SOURCE_GUI" ]] || fail "GUI build succeeded but expected artifact is missing: $SOURCE_GUI"
    deploy_binary "$SOURCE_GUI" "$REPO_ROOT/solidcraft" "GUI" "$TARGET_DIR/backup"
fi

if [[ "$BUILD_CLI" == true ]]; then
    echo "[STEP 5] Building CLI binary ($PROFILE)..."
    cargo build "${CARGO_PROFILE_FLAG[@]}" --bin solidcraft-cli "${JOBS_ARG[@]}"
    SOURCE_CLI="$TARGET_DIR/solidcraft-cli"
    [[ -f "$SOURCE_CLI" ]] || fail "CLI build succeeded but expected artifact is missing: $SOURCE_CLI"
    deploy_binary "$SOURCE_CLI" "$REPO_ROOT/solidcraft-cli" "CLI" "$TARGET_DIR/backup"
fi

if [[ "$BUILD_WEB" == true ]]; then
    echo "[STEP 6] Building WebAssembly target ($PROFILE)..."
    cargo build "${CARGO_PROFILE_FLAG[@]}" --target wasm32-unknown-unknown --bin solidcraft-web "${JOBS_ARG[@]}"
    SOURCE_WEB="$REPO_ROOT/target/wasm32-unknown-unknown/$PROFILE/solidcraft-web.wasm"
    [[ -f "$SOURCE_WEB" ]] || fail "WebAssembly build succeeded but expected artifact is missing: $SOURCE_WEB"
    mkdir -p "$WEB_INSTALL_DIR"
    WEB_STAGED="$WEB_INSTALL_DIR/solidcraft.wasm.new.$$"
    cp -- "$SOURCE_WEB" "$WEB_STAGED"
    mv -f -- "$WEB_STAGED" "$WEB_INSTALL_DIR/solidcraft.wasm"
    echo "Web deployment successful to $WEB_INSTALL_DIR."
fi

printf '%s' "$BUILD_STATE" > "$STATE_FILE.new.$$"
mv -f -- "$STATE_FILE.new.$$" "$STATE_FILE"

echo "=================================================="
echo "Update sequence completed: $(date)"
echo "=================================================="

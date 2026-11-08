#!/bin/bash

# Configuration Toggles (Modify defaults as needed to true or false to build component)
BUILD_GUI=true
BUILD_CLI=true
BUILD_WEB=true

export HOME="$(getent passwd $USER | cut -d: -f6)"
source "$HOME/.cargo/env"
REPO_DIR="$HOME/solidcraft"
WEB_DIR="/var/www/html/solidcraft"
cd "$REPO_DIR" || { echo "Failed to cd into $REPO_DIR"; exit 1; }

# Setup Log Directory and File
LOG_DIR="$REPO_DIR/build_logs"
mkdir -p "$LOG_DIR"
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
LOG_FILE="$LOG_DIR/build_$TIMESTAMP.log"

# Redirect all stdout and stderr to both the console and the log file
exec > >(tee -a "$LOG_FILE") 2>&1

echo "===================================================="
echo " Starting SolidCraft Linux Pipeline: $(date)"
echo " Log File: $LOG_FILE"
echo "===================================================="

# Track individual target statuses
GUI_SUCCESS=false
CLI_SUCCESS=false
WEB_SUCCESS=false

write_step_header() {
    echo ""
    echo "----------------------------------------------------"
    echo " [STEP $1] $2"
    echo "----------------------------------------------------"
}

write_prominent_error() {
    echo ""
    echo "####################################################"
    echo " [FATAL ERROR]: $1"
    echo "####################################################"
    echo ""
}

# ----------------------------------------------------------------
write_step_header "1" "Fetch and Pull Latest Changes from Git"
# ----------------------------------------------------------------
git fetch origin

CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)
REMOTE_COMMIT=$(git rev-parse "origin/$CURRENT_BRANCH")
LOCAL_COMMIT=$(git rev-parse HEAD)

if [ "$LOCAL_COMMIT" != "$REMOTE_COMMIT" ]; then
    echo "Changes detected on remote. Pulling latest code..."
    git pull
    if [ $? -ne 0 ]; then
        write_prominent_error "Git pull failed. Aborting entire update chain."
        exit 1
    fi
    echo "Git pull completed successfully."
else
    echo "Local repository is already up to date with origin/$CURRENT_BRANCH."
fi

# ----------------------------------------------------------------
write_step_header "2" "Run Workspace Test Suite (Debug Mode)"
# ----------------------------------------------------------------
cargo test --workspace --jobs 2
if [ $? -ne 0 ]; then
    write_prominent_error "Tests failed! Aborting deployment. Existing build remains untouched."
    exit 1
fi
echo "All tests passed successfully!"

# ----------------------------------------------------------------
write_step_header "3" "Compile Target: GUI (solidcraft)"
# ----------------------------------------------------------------
if [ "$BUILD_GUI" = true ]; then
    echo "Building solidcraft (GUI)..."
    cargo build --release -p solidcraft --jobs 2
    if [ $? -ne 0 ]; then
        write_prominent_error "GUI compilation failed!"
    else
        GUI_SUCCESS=true
        echo "GUI compilation succeeded."
    fi
else
    echo "GUI target skipped by parameter."
fi

# ----------------------------------------------------------------
write_step_header "4" "Compile Target: CLI (solidcraft-cli)"
# ----------------------------------------------------------------
if [ "$BUILD_CLI" = true ]; then
    echo "Building solidcraft-cli (CLI)..."
    cargo build --release -p solidcraft-cli --jobs 2
    if [ $? -ne 0 ]; then
        write_prominent_error "CLI compilation failed!"
    else
        CLI_SUCCESS=true
        echo "CLI compilation succeeded."
    fi
else
    echo "CLI target skipped by parameter."
fi

# ----------------------------------------------------------------
write_step_header "5" "Compile Target: Web (solidcraft-web)"
# ----------------------------------------------------------------
if [ "$BUILD_WEB" = true ]; then
    echo "Building solidcraft-web (WASM)..."
    cargo build --release -p solidcraft-web --target wasm32-unknown-unknown --jobs 2
    if [ $? -ne 0 ]; then
        write_prominent_error "Web compilation failed!"
    else
        WEB_SUCCESS=true
        echo "Web compilation succeeded."
    fi
else
    echo "Web target skipped by parameter."
fi

# ----------------------------------------------------------------
write_step_header "6" "Atomic Deployment for Successful Targets"
# ----------------------------------------------------------------

# Deploy GUI if built successfully
if [ "$BUILD_GUI" = true ] && [ "$GUI_SUCCESS" = true ]; then
    SOURCE_GUI="$REPO_DIR/target/release/solidcraft"
    TARGET_GUI="$REPO_DIR/solidcraft"
    TEMP_GUI="$REPO_DIR/solidcraft_new"

    if [ -f "$SOURCE_GUI" ]; then
        echo "Deploying GUI binary atomically..."
        cp -f "$SOURCE_GUI" "$TEMP_GUI"
        if [ -f "$TARGET_GUI" ]; then
            mv -f "$TARGET_GUI" "$REPO_DIR/solidcraft_old"
            mv -f "$TEMP_GUI" "$TARGET_GUI"
            rm -f "$REPO_DIR/solidcraft_old"
        else
            mv -f "$TEMP_GUI" "$TARGET_GUI"
        fi
        chmod +x "$TARGET_GUI"
        echo "Successfully deployed solidcraft!"
    else
        write_prominent_error "GUI binary missing at expected path despite successful build status."
    fi
fi

# Deploy CLI if built successfully
if [ "$BUILD_CLI" = true ] && [ "$CLI_SUCCESS" = true ]; then
    SOURCE_CLI="$REPO_DIR/target/release/solidcraft-cli"
    TARGET_CLI="$REPO_DIR/solidcraft-cli"
    TEMP_CLI="$REPO_DIR/solidcraft-cli_new"

    if [ -f "$SOURCE_CLI" ]; then
        echo "Deploying CLI binary atomically..."
        cp -f "$SOURCE_CLI" "$TEMP_CLI"
        if [ -f "$TARGET_CLI" ]; then
            mv -f "$TARGET_CLI" "$REPO_DIR/solidcraft-cli_old"
            mv -f "$TEMP_CLI" "$TARGET_CLI"
            rm -f "$REPO_DIR/solidcraft-cli_old"
        else
            mv -f "$TEMP_CLI" "$TARGET_CLI"
        fi
        chmod +x "$TARGET_CLI"
        echo "Successfully deployed solidcraft-cli!"
    else
        write_prominent_error "CLI binary missing at expected path despite successful build status."
    fi
fi

# Deploy Web assets to web directory if built successfully
if [ "$BUILD_WEB" = true ] && [ "$WEB_SUCCESS" = true ]; then
    SOURCE_WEB="$REPO_DIR/target/wasm32-unknown-unknown/release/solidcraft_web.wasm"

    if [ -f "$SOURCE_WEB" ]; then
        echo "Deploying web assets to web root ($WEB_DIR)..."

        # Note: Deploying to /var/www/html/ typically requires elevated privileges.
        # Ensure the cron user has passwordless sudo rights for cp/mkdir/chown, 
        # or grant the build user ownership of $WEB_DIR.

        # Ensure web directory exists (using sudo if permissions require it)
        sudo mkdir -p "$WEB_DIR"

        # Copy compiled WASM binary and any associated web artifacts
        sudo cp -f "$SOURCE_WEB" "$WEB_DIR/"

        # Optional: Set appropriate web server permissions (e.g., www-data)
        sudo chown -R www-data:www-data "$WEB_DIR" 2>/dev/null || true

        echo "Successfully deployed web assets to $WEB_DIR!"
    else
        write_prominent_error "Web WASM binary missing at expected path despite successful build status."
    fi
fi

# ----------------------------------------------------------------
write_step_header "7" "Pipeline Summary Report"
# ----------------------------------------------------------------
echo "Pipeline Execution Complete."
[ "$BUILD_GUI" = true ] && echo " - GUI Target: $( [ "$GUI_SUCCESS" = true ] && echo "SUCCESS" || echo "FAILED" )"
[ "$BUILD_CLI" = true ] && echo " - CLI Target: $( [ "$CLI_SUCCESS" = true ] && echo "SUCCESS" || echo "FAILED" )"
[ "$BUILD_WEB" = true ] && echo " - Web Target: $( [ "$WEB_SUCCESS" = true ] && echo "SUCCESS" || echo "FAILED" )"
echo "Full log saved to: $LOG_FILE"

# Exit with error if any enabled target failed
if { [ "$BUILD_GUI" = true ] && [ "$GUI_SUCCESS" = false ]; } || \
   { [ "$BUILD_CLI" = true ] && [ "$CLI_SUCCESS" = false ]; } || \
   { [ "$BUILD_WEB" = true ] && [ "$WEB_SUCCESS" = false ]; }
then
    exit 2
fi

exit 0

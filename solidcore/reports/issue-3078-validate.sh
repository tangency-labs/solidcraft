#!/usr/bin/env bash
set -uo pipefail
export CARGO_BUILD_JOBS=2
mkdir -p reports/issue-3078-validation
failed=0
run() {
    local name=$1; shift
    printf 'BEGIN %s\n' "$name"
    nice -n 10 "$@" >"reports/issue-3078-validation/$name.log" 2>&1
    local rc=$?
    printf '%s exit=%s\n' "$name" "$rc" | tee -a reports/issue-3078-validation/status.txt
    tail -12 "reports/issue-3078-validation/$name.log"
    if [ "$rc" != 0 ]; then failed=1; fi
}
: >reports/issue-3078-validation/status.txt
run fmt cargo fmt --all --check
run section-lib cargo test --profile goldens -p solidcore --features profile --lib -- blend::fillet_section
run profile-lib cargo test --profile goldens -p solidcore --features profile --lib -- profile::
run ops-blend cargo test --profile goldens -p solidcore-ops --test blend
args=()
while IFS= read -r file; do
    args+=(--test "$(basename "$file" .rs)")
done < <(grep -ilE 'variable|fillet' crates/solidcore/tests/*.rs | sort)
printf '%s\n' "${args[@]}" > reports/issue-3078-validation/integration-targets.txt
run integrations cargo test --profile goldens -p solidcore --no-fail-fast "${args[@]}" -- --test-threads=2
run clippy cargo clippy --workspace --all-targets -- -D warnings
run profile-clippy cargo clippy -p solidcore --features profile --lib --test wr685_law_corners -- -D warnings
run wasm cargo check -p solidcore --target wasm32-unknown-unknown
run panic-sites python3 -I scripts/ci/check_panic_sites.py --base f4f70127afe
run blend-goldens cargo run --profile goldens -p goldens -- --unit 'blend.*' --goldens
exit "$failed"

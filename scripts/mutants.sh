#!/usr/bin/env bash
# Two-tier mutation testing for the solidcore kernel.
#
# Tier 1 runs cargo-mutants against the package's unit tests. Tier 2 replays each
# unit-test survivor against one difftest unit (Rust port vs the C++ reference),
# because difftest — not the unit tests — is the correctness gate. A mutant that
# passes both tiers is behavior no check constrains: the useful output.
#
# Usage:
#   scripts/mutants.sh --module crates/solidcore/src/geom/vector2d.rs --unit geom.vector2d
#   scripts/mutants.sh --in-diff-base origin/main --unit <id>   # the PR's changed lines
#   scripts/mutants.sh --module ... --shard 1/20 --no-difftest  # tier 1 only
#   scripts/mutants.sh --module ... --re 'is_small_topology' --unit ...  # one mutant
#
# Tier 2 needs the C++ reference: natively on Linux with bin/cpp (scripts/fetch-cpp.sh),
# or through --docker-image on other hosts (see docs/mutation-testing.md).
#
# Output: reports/mutants/<utc-stamp>/ with cargo-mutants' mutants.out, its log, the
# survivor list and tier2.tsv. Reports are not committed (/reports/mutants is ignored).
# Exit status is 0 unless --fail-on-both-tier finds a mutant that both tiers miss.
set -uo pipefail

root=$(git rev-parse --show-toplevel)
cd "$root" || exit 1

usage() { sed -n '2,20p' "$0"; }

pkg=solidcore
module=
unit=
in_diff_base=
jobs=2
shard=1/1
timeout=180
max_replay=
mutant_re=
out=
docker_image=
docker_cpp_dir=/work/bin/cpp-linux
run_difftest=1
fail_on_both=0
list_only=0

while [ $# -gt 0 ]; do
  case "$1" in
    -m|--module) module=$2; shift 2 ;;
    -u|--unit) unit=$2; shift 2 ;;
    -D|--in-diff-base) in_diff_base=$2; shift 2 ;;
    -p|--package) pkg=$2; shift 2 ;;
    -j|--jobs) jobs=$2; shift 2 ;;
    -s|--shard) shard=$2; shift 2 ;;
    --timeout) timeout=$2; shift 2 ;;
    --max-replay) max_replay=$2; shift 2 ;;
    --re) mutant_re=$2; shift 2 ;;
    -o|--out) out=$2; shift 2 ;;
    --docker-image) docker_image=$2; shift 2 ;;
    --docker-cpp-dir) docker_cpp_dir=$2; shift 2 ;;
    --no-difftest) run_difftest=0; shift ;;
    --fail-on-both-tier) fail_on_both=1; shift ;;
    --list-only) list_only=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "mutants: unknown argument: $1" >&2; exit 2 ;;
  esac
done

if [ -z "$module" ] && [ -z "$in_diff_base" ]; then
  echo "mutants: give --module FILE or --in-diff-base REF" >&2
  exit 2
fi
if [ "$list_only" = 0 ] && [ "$run_difftest" = 1 ] && [ -z "$unit" ]; then
  echo "mutants: --unit is required for tier 2 (or pass --no-difftest)" >&2
  exit 2
fi
command -v cargo-mutants >/dev/null 2>&1 || {
  echo "mutants: cargo-mutants is not installed (cargo install cargo-mutants)" >&2
  exit 2
}

# The repository pins its toolchain; on machines where another cargo shadows rustup
# (e.g. Homebrew on macOS), prefer the pinned toolchain rustup has installed.
channel=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml 2>/dev/null | head -1)
if [ -n "${channel:-}" ] && command -v rustup >/dev/null 2>&1; then
  tc=$(rustup toolchain list 2>/dev/null \
    | awk -v c="$channel" 'index($1, c "-") == 1 { print $1; exit }')
  tcbin="$HOME/.rustup/toolchains/${tc:-}/bin"
  if [ -n "${tc:-}" ] && [ -d "$tcbin" ]; then
    case ":$PATH:" in *":$tcbin:"*) ;; *) PATH="$tcbin:$PATH" ;; esac
    export PATH RUSTC="$tcbin/rustc"
    if [ -d "$tcbin/../lib" ]; then
      export DYLD_FALLBACK_LIBRARY_PATH="$tcbin/../lib${DYLD_FALLBACK_LIBRARY_PATH:+:$DYLD_FALLBACK_LIBRARY_PATH}"
    fi
  fi
fi

stamp=$(date -u +%Y%m%dT%H%M%SZ)
out=${out:-$root/reports/mutants/$stamp}
mkdir -p "$out"
log=$out/cargo-mutants.log

# Shared-machine budget: cap the child builds and run them at low priority.
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2}
NICE=
command -v nice >/dev/null 2>&1 && NICE="nice -n 10"

args=(-p "$pkg" --baseline run -j "$jobs" --timeout "$timeout" -o "$out")
[ -n "$module" ] && args+=(-f "$module")
[ -n "$mutant_re" ] && args+=(-F "$mutant_re")
[ "$shard" != "1/1" ] && args+=(--shard "$shard" --sharding round-robin)
if [ -n "$in_diff_base" ]; then
  if ! git rev-parse --verify --quiet "$in_diff_base^{commit}" >/dev/null; then
    echo "mutants: unknown git ref: $in_diff_base" >&2
    exit 2
  fi
  git diff "$in_diff_base...HEAD" >"$out/in-diff.patch"
  args+=(--in-diff "$out/in-diff.patch")
fi
[ "$list_only" = 1 ] && args+=(--list)

echo "mutants: tier 1: cargo mutants ${args[*]}" | tee "$log"
# shellcheck disable=SC2086 # NICE is an optional "nice -n 10" prefix.
$NICE cargo mutants "${args[@]}" 2>&1 | tee -a "$log"
rc=${PIPESTATUS[0]}
if [ "$list_only" = 1 ]; then
  exit "$rc"
fi
# cargo-mutants: 0 all caught, 2 missed mutants, 3 timeouts are normal outcomes;
# anything else (usage, baseline failure, in-diff mismatch, internal) is fatal.
case "$rc" in
  0 | 2 | 3) : ;;
  *) echo "mutants: tier 1 failed (exit $rc); see $log" >&2; exit "$rc" ;;
esac

mo=$out/mutants.out
if [ ! -d "$mo" ]; then
  echo "mutants: no $mo (did the baseline fail?)" >&2
  exit 1
fi
count() { [ -f "$mo/$1.txt" ] && wc -l <"$mo/$1.txt" | tr -d ' ' || echo 0; }
caught=$(count caught)
missed=$(count missed)
timeout=$(count timeout)
unviable=$(count unviable)
echo "mutants: tier 1: caught=$caught missed=$missed timeout=$timeout unviable=$unviable"

both=0
if [ "$run_difftest" = 1 ] && [ "$missed" -gt 0 ]; then
  # Mutated files must be clean: tier 2 patches the working tree.
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    f=${line%%:*}
    if [ -n "$(git status --porcelain -- "$f")" ]; then
      echo "mutants: $f has local changes; tier 2 patches the working tree" >&2
      exit 2
    fi
  done <"$mo/missed.txt"

  cname=
  applied=
  cleanup() {
    if [ -n "${applied:-}" ]; then
      patch -R -p0 <"$applied" >/dev/null 2>&1
    fi
    if [ -n "${cname:-}" ]; then
      docker rm -f "$cname" >/dev/null 2>&1
    fi
  }
  if [ -n "$docker_image" ]; then
    command -v docker >/dev/null 2>&1 || { echo "mutants: docker not found" >&2; exit 2; }
    # One container for every survivor: the kernel rebuild is cached between runs.
    cname=solidcore-mutants-$$
    docker run -d --name "$cname" -v "$root:/work" -w /work "$docker_image" sleep infinity >/dev/null \
      || { echo "mutants: cannot start $docker_image" >&2; exit 2; }
    trap cleanup EXIT
    run_unit() {
      docker exec "$cname" bash -lc \
        "export PATH=\$HOME/.cargo/bin:\$PATH SOLIDCORE_CPP_DIR=$docker_cpp_dir CARGO_TARGET_DIR=/tmp/sc-target CARGO_BUILD_JOBS=2; nice -n 10 cargo run --profile difftest -p difftest --features cpp -- --unit $unit"
    }
  else
    trap cleanup EXIT
    cpp=${SOLIDCORE_CPP_DIR:-$root/bin/cpp}
    if [ ! -f "$cpp/lib/libSMLib.a" ]; then
      echo "mutants: no C++ reference at $cpp (scripts/fetch-cpp.sh); tier 2 skipped" >&2
      echo "mutants: pass --docker-image or --no-difftest" >&2
      exit 2
    fi
    run_unit() {
      # shellcheck disable=SC2086 # NICE is an optional "nice -n 10" prefix.
      SOLIDCORE_CPP_DIR="$cpp" CARGO_BUILD_JOBS=2 $NICE \
        cargo run --profile difftest -p difftest --features cpp -- --unit "$unit"
    }
  fi

  tier2=$out/tier2.tsv
  : >"$tier2"
  n=0
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    n=$((n + 1))
    [ -n "$max_replay" ] && [ "$n" -gt "$max_replay" ] && break
    path=${line%%:*}
    rest=${line#*:}
    ln=${rest%%:*}
    col=$(printf '%s' "$rest" | cut -d: -f2)
    key=$(printf '%s' "$path" | sed 's|/|__|g')_line_${ln}_col_${col}.diff
    diff=$mo/diff/$key
    if [ ! -f "$diff" ]; then
      printf '%s\tno-diff\t\n' "$line" >>"$tier2"
      continue
    fi
    if ! patch -p0 <"$diff" >/dev/null; then
      printf '%s\tpatch-fail\t\n' "$line" >>"$tier2"
      continue
    fi
    applied=$diff
    summary=$(run_unit 2>&1 | grep -E '^(PASS|FAIL|PARTIAL) ' | head -1)
    case "$summary" in
      PASS*) verdict=both-tier-missed; both=$((both + 1)) ;;
      FAIL* | PARTIAL*) verdict=caught-by-difftest ;;
      *) verdict=unknown ;;
    esac
    printf '%s\t%s\t%s\n' "$line" "$verdict" "${summary:-no-summary}" >>"$tier2"
    echo "mutants: $verdict: $line"
    patch -R -p0 <"$diff" >/dev/null || echo "mutants: revert failed for $line" >&2
    applied=
  done <"$mo/missed.txt"
fi

{
  echo "unit=$unit module=$module shard=$shard in_diff=$in_diff_base max_replay=${max_replay:-all}"
  echo "caught=$caught missed=$missed timeout=$timeout unviable=$unviable both-tier=$both"
  if [ "$run_difftest" = 1 ]; then echo "tier2=run"; else echo "tier2=skipped"; fi
} >"$out/summary.txt"
echo "mutants: both tiers missed $both mutant(s); report: ${out#"$root"/}"

if [ "$fail_on_both" = 1 ] && [ "$both" -gt 0 ]; then
  exit 1
fi

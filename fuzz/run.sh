#!/bin/bash
# Runs every fuzz target for a while and reports which ones found something.
#   fuzz/run.sh [seconds per target, default 60] [target ...]
# Needs a nightly toolchain and cargo-fuzz (see README.md). New inputs collect in
# fuzz/corpus/<target>; a finding is saved under fuzz/artifacts/<target>/.
cd "$(dirname "$0")" || exit 1
seconds="${1:-60}"
[ $# -gt 0 ] && shift
targets=("$@")
if [ ${#targets[@]} -eq 0 ]; then
  # (No `mapfile`: macOS ships bash 3.2.)
  while IFS= read -r target; do
    targets+=("$target")
  done < <(cargo +nightly fuzz list)
fi
cargo +nightly fuzz build || exit 1
failed=()
for target in "${targets[@]}"; do
  mkdir -p "corpus/$target"
  echo "== $target"
  # No decoder may take seconds or hundreds of megabytes for 64 KiB of input. The
  # one exception: a planet's picture may be 16 384 x 8192 pixels, 512 MiB decoded.
  malloc_mb=256
  [ "$target" = planet ] && malloc_mb=600
  if ! cargo +nightly fuzz run "$target" "corpus/$target" "seeds/$target" -- \
      -max_total_time="$seconds" -timeout=10 -max_len=65536 \
      -malloc_limit_mb="$malloc_mb" -rss_limit_mb=2048 -print_final_stats=1 \
      > "target/$target.log" 2>&1; then
    failed+=("$target")
    grep -E "panicked at|ERROR: |SUMMARY: |Test unit written" "target/$target.log" | head -6
  else
    grep -E "stat::number_of_executed_units|stat::new_units_added" "target/$target.log"
  fi
done
if [ ${#failed[@]} -gt 0 ]; then
  echo "FINDINGS in: ${failed[*]} (logs in fuzz/target/<target>.log)"
  exit 1
fi
echo "No findings in ${#targets[@]} targets."

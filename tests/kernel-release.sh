#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
scratch=$(mktemp -d)
trap 'rm -r -- "$scratch"' EXIT
export CALL_LOG="$scratch/calls"
cat > "$scratch/cargo" <<'SH'
#!/usr/bin/env bash
printf 'cargo %s\n' "$*" >> "$CALL_LOG"
exit "${BUILD_STATUS:-0}"
SH
cat > "$scratch/elf2uf2-rs" <<'SH'
#!/usr/bin/env bash
printf 'deploy %s\n' "$*" >> "$CALL_LOG"
SH
chmod +x "$scratch/cargo" "$scratch/elf2uf2-rs"
export PATH="$scratch:$PATH"
unset BUILD_STATUS

for board in rp235xa pimoroni2w; do
    for clock in default stable 300; do
        : > "$CALL_LOG"
        args=("$board")
        features="$board"
        if [[ "$clock" != default ]]; then args+=("$clock"); fi
        if [[ "$clock" == 300 ]]; then features+=",overclock-300"; fi
        just kernel-release "${args[@]}"
        expected=$(printf 'cargo build --bin kernel --release --no-default-features --features %s\ndeploy -d target/thumbv8m.main-none-eabihf/release/kernel' "$features")
        [[ "$(cat "$CALL_LOG")" == "$expected" ]]
    done
done

: > "$CALL_LOG"
if just kernel-release rp235xa '300; exit 0'; then exit 1; fi
[[ ! -s "$CALL_LOG" ]]

: > "$CALL_LOG"
if BUILD_STATUS=1 just kernel-release rp235xa 300; then exit 1; fi
[[ $(wc -l < "$CALL_LOG") -eq 1 ]]
printf 'kernel-release checks passed (mocked build/deploy)\n'

target := "thumbv8m.main-none-eabihf"

kernel-dev board:
    cargo run --bin kernel --features {{board}}  --features fps --features defmt
kernel-release-probe board:
    cargo run --bin kernel --profile release --features {{board}} --features fps
kernel-release board:
    cargo build --bin kernel --release --no-default-features --features {{board}}
    elf2uf2-rs -d target/{{target}}/release/kernel

binary-args := "RUSTFLAGS=\"-C link-arg=-pie -C relocation-model=pic\""

cbindgen:
    mkdir -p target/generated
    cbindgen userlib_sys --config tools/build/cbindgen.toml --output target/generated/userlib_sys.h -q

check-header: cbindgen
    printf '#include <stddef.h>\n#include "userlib_sys.h"\n_Static_assert(sizeof(struct KeyEventC) == 3, "key event size");\n_Static_assert(offsetof(struct KeyEventC, state) == 1, "key state offset");\n_Static_assert(offsetof(struct KeyEventC, mods) == 2, "key modifiers offset");\n' | cc -std=c11 -x c -Itarget/generated -fsyntax-only -
    printf '#include "userlib_sys.h"\n' | c++ -std=c++17 -x c++ -Itarget/generated -fsyntax-only -

newlib:
    #!/bin/bash
    set -euo pipefail
    cd user_apps/picolibc
    mkdir -p build
    cd build
    CONFIG_PICOLIBC=true ../scripts/do-configure thumbv8m_main_fp-none-eabi \
        --buildtype=minsize \
        -Dtests=true \
        -Dtinystdio=false \
        -Dsingle-thread=true \
        -Db_pie=true \
        -Ddefault_library=static \
        -Dtinystdio=false \
        -Dnewlib-nano-malloc=true \
        -Dmultilib=false \
        -Dpicolib=true \
        "$@" 
    DESTDIR=./install meson install
    ninja

userapp app:
     {{binary-args}} cargo build --bin {{app}} --profile release-binary

doom:
    python3 user_apps/doom/build.py

userapps: cbindgen
    just userapp calculator
    just userapp snake
    just userapp gallery
    just userapp gif
    just userapp wav_player

# gvfs backends usually live on the host, not inside the toolbox, so fall back
# to running gio there. MTP is spoken by the gvfs daemon, not by gio itself.
gio := "$(if command -v gio >/dev/null 2>&1 && [ -e /usr/libexec/gvfsd-mtp ]; then echo gio; else echo 'flatpak-spawn --host gio'; fi)"

# Root URI of the running kernel's MTP responder.
[private]
mtp-uri:
    #!/bin/bash
    set -euo pipefail
    devices=$({{gio}} mount -li)
    dev=$(printf '%s\n' "$devices" | awk 'match($0, /mtp:\/\/LegitCamper_PicoCalc_01001100\/?([[:space:]]|$)/) { uri=substr($0, RSTART, RLENGTH); sub(/[[:space:]]+$/, "", uri); print uri; exit }')
    if [ -z "$dev" ]; then
        echo "PicoCalc MTP device not found; is the kernel running and USB plugged in?" >&2
        exit 1
    fi
    dev="${dev%/}"
    # gvfs lists the device before it is mounted; copying needs it mounted.
    if ! {{gio}} list "$dev" >/dev/null 2>&1; then
        {{gio}} mount "$dev"
    fi
    echo "$dev/SD Card"

# On-card name for an app. FAT 8.3 allows an 8-char base, and the kernel
# cannot write long-file-name entries, so longer app names need an alias.
# The app picker lists whatever is on the card, so this is the name it shows.
[private]
[positional-arguments]
sd-name app:
    #!/bin/bash
    set -euo pipefail
    export LC_ALL=C
    case "$1" in
        calculator) base=CALC ;;
        wav_player) base=WAVPLAY ;;
        *) base=$(printf '%s' "$1" | tr '[:lower:]' '[:upper:]') ;;
    esac
    if [[ ! "$base" =~ ^[A-Z0-9_]{1,8}$ ]]; then
        echo "'$1' needs a valid <=8 char alias in the sd-name recipe" >&2
        exit 1
    fi
    echo "$base.BIN"

# Overwrite one app on the SD card over MTP. The responder rejects
# SendObjectInfo for an existing name (RC_INVALID_PARAMETER), so delete first.
# Delete by the on-card name: MTP exposes the uppercase 8.3 name, and
# removing the lowercase spelling silently does nothing.
[positional-arguments]
copy-userapp app:
    #!/bin/bash
    set -euo pipefail
    name="$(just sd-name "$1")"
    source="$(realpath -e "./target/{{target}}/release-binary/$1")"
    just copy-sd-file "$source" "$name"
    shopt -s nullglob
    case "$1" in
        gallery)
            for asset in assets/gallery/*.bmp; do
                name="${asset##*/}"
                just copy-sd-file "$asset" "${name/Sprite-/IMG}"
            done ;;
        gif)
            for asset in assets/gif/*.gif; do
                name="${asset##*/}"
                just copy-sd-file "$asset" "${name/bad_apple/BADAPPLE}"
            done ;;
        wav_player)
            for asset in assets/wav_player/*.wav; do
                just copy-sd-file "$asset" "${asset##*/}"
            done ;;
    esac

[private]
[positional-arguments]
copy-sd-file source name:
    #!/bin/bash
    set -euo pipefail
    export LC_ALL=C
    source="$(realpath -e "$1")"
    name=$(printf '%s' "$2" | tr '[:lower:]' '[:upper:]')
    if [[ ! "$name" =~ ^[A-Z0-9_]{1,8}\.[A-Z0-9_]{1,3}$ ]]; then
        echo "'$2' needs an ASCII 8.3 filename" >&2
        exit 1
    fi
    if [[ ! -f "$source" || ! -r "$source" || ! -s "$source" ]]; then
        echo "missing, unreadable, or empty file: $source" >&2
        exit 1
    fi
    uri="$(just mtp-uri)"
    entries=$({{gio}} list "$uri")
    existing=$(printf '%s\n' "$entries" | LC_ALL=C awk -v name="$name" 'toupper($0) == name { print; exit }')
    transfer_dir=$(mktemp -d "$PWD/target/mtp-transfer.XXXXXX")
    echo "Transfer backup/readback: $transfer_dir"
    if [[ -n "$existing" ]]; then
        {{gio}} copy "$uri/$existing" "$transfer_dir/$name"
        if cmp -s "$source" "$transfer_dir/$name"; then
            rm -r -- "$transfer_dir"
            echo "Verified $name (unchanged)"
            exit 0
        fi
        {{gio}} remove "$uri/$existing"
    fi
    {{gio}} copy "$source" "$uri/$name"
    {{gio}} copy "$uri/$name" "$transfer_dir/readback.bin"
    cmp "$source" "$transfer_dir/readback.bin"
    rm -r -- "$transfer_dir"
    echo "Verified $name"

copy-userapps:
    #!/bin/bash
    set -euo pipefail
    just userapps
    for app in calculator snake gallery gif wav_player; do
        just copy-userapp "$app"
    done

# Host-only logic; no probe or SD card needed.
test:
    cargo test -p userlib_sys -p userlib -p selection_ui -p tinygif --target x86_64-unknown-linux-gnu
    python3 tests/test_framebuffer.py
    rustc --edition 2024 --test kernel/src/elf_format.rs -o target/elf-format-tests
    target/elf-format-tests
    rustc --edition 2024 --test user_apps/wav_player/src/wav_format.rs -o target/wav-format-tests
    target/wav-format-tests
    rustc --edition 2024 --test user_apps/snake/src/timing.rs -o target/snake-timing-tests
    target/snake-timing-tests
    rustc --edition 2024 --test kernel/src/mtp/codec.rs -o target/mtp-codec-tests
    target/mtp-codec-tests
    rustc --edition 2024 --test kernel/src/mtp/handles.rs -o target/mtp-handle-tests
    target/mtp-handle-tests
    python3 tests/test_mtp_copy.py
    python3 tests/test_build_layout.py

# The XIP-down PSRAM init lives in .data; a call into flash from there
# bus-faults on the instruction prefetch. Only the critical-section pair is
# allowed, since it runs outside the window where XIP is disabled.
check-xip-safe board:
    #!/bin/bash
    set -euo pipefail
    cargo build --bin kernel --release --no-default-features --features {{board}},psram
    bad=$(arm-none-eabi-objdump -d --section=.data \
        target/{{target}}/release/kernel \
        | grep -E '\s(bl|blx)\s' | sed 's/.*<//;s/>.*//' | sort -u \
        | grep -v 'detect_psram_qmi' | grep -v 'critical_section_impl' || true)
    if [ -n "$bad" ]; then
        echo "flash calls from .data PSRAM init:" >&2
        echo "$bad" >&2
        exit 1
    fi

# everything a change must pass before review
check: test check-header
    cargo fmt --check
    cargo build --bin kernel --release --no-default-features --features rp235xa
    just check-xip-safe pimoroni2w
    just userapps
    target/elf-format-tests --ignored rebuilt_apps_match_loader_contract

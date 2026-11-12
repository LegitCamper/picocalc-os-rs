target := "thumbv8m.main-none-eabihf"

kernel-dev board:
    cargo run --bin kernel --features {{board}}  --features fps --features defmt
kernel-release-probe board:
    cargo run --bin kernel --profile release --features {{board}} --features fps
[positional-arguments]
kernel-release board clock='stable':
    #!/usr/bin/env bash
    set -euo pipefail
    features="$1"
    case "$2" in
        stable) ;;
        300) features+=",overclock-300" ;;
        *) printf 'Unsupported clock: %s (expected stable or 300)\n' "$2" >&2; exit 2 ;;
    esac
    cargo build --bin kernel --release --no-default-features --features "$features"
    elf2uf2-rs -d target/{{target}}/release/kernel

binary-args := "RUSTFLAGS=\"-C link-arg=-pie -C relocation-model=pic\""

cbindgen:
    cbindgen userlib_sys --output userlib_sys.h -q

newlib:
    #!/bin/bash
    cd picolibc
    mkdir build
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

userapps: cbindgen
    just userapp calculator
    just userapp snake
    just userapp gallery
    just userapp gif
    just userapp wav_player
    just userapp gboy

copy-userapp app:
    cp ./target/{{target}}/release-binary/{{app}} /run/media/$(whoami)/PICOCALC/{{app}}.bin

copy-userapps:
    #!/bin/bash
    just userapps
    just copy-userapp calculator
    just copy-userapp snake
    just copy-userapp gallery
    just copy-userapp gif
    just copy-userapp wav_player
    just copy-userapp gboy

    DEV=$(lsblk -o LABEL,NAME -nr | awk -v L="PICOCALC" '$1==L {print "/dev/" $2}')
    udisksctl unmount -b "$DEV"
    udisksctl power-off -b "$DEV"    

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
    cbindgen userlib_sys --config cbindgen.toml --output userlib_sys.h -q

check-header: cbindgen
    printf '#include "userlib_sys.h"\n' | cc -std=c11 -x c -I. -fsyntax-only -
    printf '#include "userlib_sys.h"\n' | c++ -std=c++17 -x c++ -I. -fsyntax-only -

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

doom:
    python3 user_apps/doom/build.py

userapps: cbindgen
    just userapp calculator
    just userapp snake
    just userapp gallery
    just userapp gif
    just userapp wav_player

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

    DEV=$(lsblk -o LABEL,NAME -nr | awk -v L="PICOCALC" '$1==L {print "/dev/" $2}')
    udisksctl unmount -b "$DEV"
    udisksctl power-off -b "$DEV"    

# Host-only logic; no probe or SD card needed.
test:
    cargo test -p userlib_sys -p userlib -p selection_ui -p tinygif --target x86_64-unknown-linux-gnu
    rustc --edition 2024 --test kernel/src/elf_format.rs -o target/elf-format-tests
    target/elf-format-tests
    rustc --edition 2024 --test user_apps/wav_player/src/wav_format.rs -o target/wav-format-tests
    target/wav-format-tests
    rustc --edition 2024 --test kernel/src/mtp/codec.rs -o target/mtp-codec-tests
    target/mtp-codec-tests

# everything a change must pass before review
check: test check-header
    cargo fmt --check
    cargo build --bin kernel --release --no-default-features --features rp235xa
    just userapps
    target/elf-format-tests --ignored rebuilt_apps_match_loader_contract

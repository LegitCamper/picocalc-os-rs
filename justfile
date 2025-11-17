kernel-dev board:
    cargo run --bin kernel --features {{board}}  --features fps
kernel-release-probe board:
    cargo run --bin kernel --profile release --features {{board}}  --features fps
kernel-release board:
    cargo build --bin kernel --release --no-default-features --features {{board}}
    elf2uf2-rs -d target/thumbv8m.main-none-eabihf/release/kernel

binary-args := "RUSTFLAGS=\"-C link-arg=-pie -C relocation-model=pic\""

cbindgen:
    cbindgen abi_sys --output abi_sys.h -q

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

copy-userapp app:
    cp ./target/thumbv8m.main-none-eabihf/release-binary/{{app}} /run/media/$(whoami)/PICOCALC/{{app}}.bin

copy-userapps:
    #!/bin/bash
    just userapps
    just copy-userapp calculator
    just copy-userapp snake
    just copy-userapp gallery
    just copy-userapp gif

    DEV=$(lsblk -o LABEL,NAME -nr | awk -v L="PICOCALC" '$1==L {print "/dev/" $2}')
    udisksctl unmount -b "$DEV"
    udisksctl power-off -b "$DEV"    

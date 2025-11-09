kernel-dev board:
    cargo run --bin kernel --features {{board}} --features fps
kernel-release board:
    cargo build --bin kernel --release --no-default-features --features {{board}}
    elf2uf2-rs -d target/thumbv8m.main-none-eabihf/release/kernel

binary-args := "RUSTFLAGS=\"-C link-arg=-pie -C relocation-model=pic\""

cbindgen:
    cbindgen abi_sys --output abi_sys.h -q

userapp app:
     {{binary-args}} cargo build --bin {{app}} --profile release-binary

userapps: cbindgen
    just userapp calculator
    just userapp snake
    just userapp gallery
    just userapp gif

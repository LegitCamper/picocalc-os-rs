binary-args := "RUSTFLAGS=\"-C link-arg=-pie -C relocation-model=pic\""

kernel:
    cargo run --bin kernel
calculator:
    {{binary-args}} cargo build --bin calculator --profile release-binary
snake:
     {{binary-args}} cargo build --bin snake --profile release-binary

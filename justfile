kernel:
    cargo run --bin kernel
calculator:
    RUSTFLAGS="-C link-arg=--noinhibit-exec" cargo build --bin calculator --profile release-binary
snake:
    RUSTFLAGS="-C link-arg=--noinhibit-exec" cargo build --bin snake --profile release-binary

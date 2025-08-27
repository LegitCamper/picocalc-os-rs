kernel: calculator
    cargo run --bin kernel
calculator:
    RUSTFLAGS="-C link-arg=--noinhibit-exec" cargo build --bin calculator --release 

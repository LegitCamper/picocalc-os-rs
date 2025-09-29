kernel:
    cargo run --bin kernel

binary-args := "RUSTFLAGS=\"-C link-arg=-pie -C relocation-model=pic\""

userapp app:
     {{binary-args}} cargo build --bin {{app}} --profile release-binary

userapps:
    just userapp calculator
    just userapp snake
    just userapp gallery

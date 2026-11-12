# Local decoder safety patch

Source: `LegitCamper/tinygif`, commit
`083c0efbdbeb21835de30425e234247340f6e370`, version 0.0.4.
Original MIT and Apache licenses are retained.

This copy validates GIF metadata, bounds LZW dictionary and reconstruction
buffers, stops at the end code, and exposes `Frame::draw_checked` so corrupt
compressed data returns an error instead of panicking during playback.
No new decoder dependency or unsafe code is added.
Dictionary and reconstruction storage use fallible heap allocation: the complete
4096-entry dictionary does not fit on the kernel's 16 KiB core1 stack. Allocation
failure is reported as `ParseError::OutOfMemory`.

Run regression tests with:

```sh
cargo test -p tinygif --target x86_64-unknown-linux-gnu
```

Replace this local copy once equivalent fixes are available in the pinned
upstream version; do not remove malformed-input regressions during upgrades.

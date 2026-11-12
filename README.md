# PicoCalc OS (Rust)

A simple kernel and applications for the **Clockwork PicoCalc**, written in Rust.  
This project provides a minimal kernel, syscall table, and user-space applications to experiment with kernel development on constrained hardware.

## Status

Basic synchronous applications are working great.  
Current focus is on exanding applications and porting software, finding bugs in ffi, and making sure the kernel is as stable as possible.

## Project Structure

- **`kernel/`** – The core OS kernel
- **`userlib_sys/`** – C FFI bindings for kernel syscall
- **`userlib/`** – Rust wrapper on top of `userlib_sys` 
- **`picolib/`** – Built with ```just newlib```, and provides libc symbols when linking with C libraries 
- **`user_apps/`** – Collection of userspace programs (gif player, wav player, calculator, snake, etc.)

## Features

- Minimal Rust-based kernel targeting the PicoCalc
- Custom ABI for *Mostly* safe communication between kernel and applications
- Support for multiple user-space applications
- Hardware drivers tailored for the PicoCalc( Audio, Display, Keyboard, ans Storage )

## Getting Started

Launcher: Up/Down selects a program; Enter/Right launches it. Del prompts to
permanently delete the selected `.bin` from the SD card; Enter confirms and Esc
cancels. Failed deletions show an error and keep the program in the list.

The current `embedded-sdmmc` 0.9 delete API removes the directory entry but does
not reclaim the file's allocated clusters. Reclaim space using a computer.

```bash
git clone https://github.com/LegitCamper/picocalc-os-rs.git
cd picocalc-os-rs
just userapps
# copy the build applications from target/thumbv8m.main-none-eabihf/release-binary/application to the sdcard and rename them to app.bin

# has builds for the official rp2350 board and the pimoroni2w board
just kernel-release rp235xa # keep in mind that https://github.com/StripedMonkey/elf2uf2-rs version is required until https://github.com/JoNil/elf2uf2-rs/pull/41 is merged
```

## USB File Transfer

USB exposes an MTP device named **PicoCalc**, with storage named **SD Card**.
Uploads currently target the SD Card root and require ASCII 8.3 filenames
(up to eight characters plus a three-character extension, such as `snake.bin`).
Subdirectory uploads and long filenames are rejected, not silently renamed.
New uploads cannot truncate existing files. Use a new name to preserve an older copy.

Connect a data cable to the Pico board's native USB port; the debug probe alone
does not provide MTP. Leave the device at its launcher (MTP pauses while an app
runs). In an MTP-capable file manager, open **PicoCalc > SD Card** and copy files
into that root folder. This is file transfer, not a USB disk or drive letter.

Unmount MTP before running the hardware checks (Python 3 and libusb required):

```bash
python3 tools/mtp-smoke.py               # read-only discovery and listing
python3 tools/mtp-smoke.py --write-check # creates/removes unique test files
rustc --test --edition 2024 kernel/src/mtp/codec.rs -o /tmp/mtp_codec && /tmp/mtp_codec
```

## Validation and ABI compatibility

Run `just test` for host regression tests, or `just check` for tests, generated
C/C++ header checks, formatting, the kernel, all apps, and validation of the
rebuilt ELF files. Do not use `cargo check --workspace`: kernel and app linker
arguments differ.

The syscall table now has 18 slots, including `GetLastError`. Rebuild the kernel,
all apps and any C bindings together; old 17-slot binaries are rejected with an
ABI mismatch message. Filesystem and audio wrappers return `Result` instead of
hiding failures. Audio submission copies one 2048-byte packet and returns `Busy`
rather than waiting indefinitely. Retry when `audio_buffer_ready()` is true.

The kernel checks buffer ranges, overlaps, RNG tags and allocation ownership.
Up to 256 simultaneous app allocations are tracked and reclaimed on normal
return. Display access is serialized through DMA completion. Invalid ELF files
and recoverable media errors return to the launcher instead of panicking.

These checks are not a security sandbox: apps still execute privileged native
code, and there is no separate app byte quota. Panic handlers park core1 using
timed waits; they do not return to the menu. A panicked or non-returning app still
requires a device reset. Hardware behavior has not been validated by host tests.

Implementation status and ABI-v2 design/prototype: [plans/README.md](plans/README.md).

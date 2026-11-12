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

```bash
git clone https://github.com/LegitCamper/picocalc-os-rs.git
cd picocalc-os-rs
just userapps
# copy the build applications from target/thumbv8m.main-none-eabihf/release-binary/application to the sdcard and rename them to app.bin

# has builds for the official rp2350 board and the pimoroni2w board
just kernel-release rp235xa # keep in mind that https://github.com/StripedMonkey/elf2uf2-rs version is required until https://github.com/JoNil/elf2uf2-rs/pull/41 is merged
```

## Quick Commands

Run from this worktree's root. Kernel release commands build and deploy:

```bash
just kernel-release rp235xa          # official board, stable default
just kernel-release rp235xa 300      # official board, 300 MHz
just kernel-release pimoroni2w       # Pimoroni board, stable default
just kernel-release pimoroni2w 300   # Pimoroni board, 300 MHz
just kernel-release pimoroni2w stable # explicitly return to stable

just userapp gboy                    # build Game Boy app
just copy-userapp gboy               # copy existing build to mounted PICOCALC
just userapps                       # build all user apps
just copy-userapps                   # build/copy all, then unmount/power off PICOCALC

just kernel-dev rp235xa              # debug probe: development build, FPS + logs
just kernel-release-probe rp235xa    # debug probe: release build + FPS
```

The copy commands expect the card mounted at `/run/media/$(whoami)/PICOCALC`.
The probe commands require a connected debug probe.

### CPU Clock

`kernel-release` defaults to the board's normal clock configuration.
Both modes build and deploy. Unsupported clock values fail before building.
The `300` option enables Cargo feature `overclock-300`; the existing `overclock`
feature remains an alias. Stable mode does not enable either feature.

300 MHz is an overclock, not a stability guarantee. If the device becomes
unstable, rebuild and deploy with `stable`.

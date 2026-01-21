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
just kernel-release rp235x # keep in mind that https://github.com/StripedMonkey/elf2uf2-rs version is required until https://github.com/JoNil/elf2uf2-rs/pull/41 is merged

# PicoCalc OS (Rust)

A simple operating system for the **Clockwork PicoCalc**, written in Rust.  
This project provides a minimal kernel, ABI, and user-space applications to experiment with OS development on constrained hardware.

## Status

Basic synchronous applications are working great.  
Current focus is on **expanding the ABI syscalls** and **fixing the MSC/USB-SCSI driver** to make application development easier and smoother.

## Project Structure

- **`kernel/`** – The core OS kernel (task scheduling, drivers, memory, etc.)
- **`abi_sys/`** – Shared application binary interface definitions for kernel ↔ userspace (Repr "C")
- **`abi/`** – Rust focused ABI helpers and abstractions for easier development
- **`user-apps/`** – Collection of userspace programs (calculator, snake, etc.)

## Features

- Minimal Rust-based kernel targeting the PicoCalc
- Custom ABI for safe communication between kernel and applications
- Support for multiple user-space applications
- Hardware drivers tailored for the PicoCalc

## Getting Started

```bash
git clone https://github.com/LegitCamper/picocalc-os-rs.git
cd picocalc-os-rs
just userapps
# copy the build applications from target/thumbv8m.main-none-eabihf/release-binary/application to the sdcard and rename them to app.bin
just kernel-release # keep in mind that https://github.com/StripedMonkey/elf2uf2-rs version is required until https://github.com/JoNil/elf2uf2-rs/pull/41 is merged

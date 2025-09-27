# PicoCalc OS (Rust)

A simple operating system for the **Clockwork PicoCalc**, written in Rust.  
This project provides a minimal kernel, ABI, and user-space applications to experiment with OS development on constrained hardware.

## Project Structure

- **`kernel/`** – The core OS kernel (task scheduling, drivers, memory, etc.)
- **`abi/`** – Shared application binary interface definitions for kernel ↔ userspace interaction
- **`abi_sys/`** – System-level ABI helpers
- **`shared/`** – Shared utilities and common code across kernel and user applications
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
just calculator
# copy the calculator to the sdcard and rename it to calculator.bin
just kernel

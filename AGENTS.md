# AGENTS.md

## What this is

Bare-metal Rust OS for the Clockwork PicoCalc. Target: RP2350 (Cortex-M33,
`thumbv8m.main-none-eabihf`). Kernel runs on core0; a single dynamically
loaded user app (PIE ELF32 from the SD card) runs on core1 and calls the
kernel through a table of `extern "C"` function pointers.

## Layout

- `kernel/` — the OS: drivers (display, audio, keyboard, SD), ELF loader
  (`src/elf.rs`), syscall implementations (`src/syscalls.rs`).
- `userlib_sys/` — the ABI: `SyscallTable` enum, `#[repr(C)]` types, raw
  `extern "C"` stubs. Shared, host-testable boundary logic.
- `userlib/` — safe Rust wrapper over `userlib_sys` that apps use.
- `selection_ui/` — shared list-picker widget for apps.
- `user_apps/*` — the apps (calculator, snake, gallery, gif, wav_player).
- `picolibc/` — git submodule, libc for C code (`just newlib`).

## Commands

| Purpose          | Command                                                                 |
|------------------|-------------------------------------------------------------------------|
| Host unit tests  | `just test`                                                             |
| Full gate        | `just check` (tests + fmt + kernel build + all apps)                    |
| Kernel build     | `cargo build --bin kernel --release --no-default-features --features rp235xa` |
| Kernel UF2       | `just kernel-release rp235xa` (or `pimoroni2w`)                         |
| Build all apps   | `just userapps`                                                         |
| Build one app    | `just userapp gif`                                                      |
| Copy to SD       | `just copy-userapps`                                                    |
| Regen C header   | `just cbindgen`                                                         |
| Debug w/ probe   | `just kernel-dev rp235xa`                                               |

## Hard rules

- **User apps are untrusted input.** Every pointer, length and path that
  crosses a syscall must be validated kernel-side. A syscall must never panic
  on bad arguments — return the function's "nothing happened" value and
  `defmt::warn!`.
- **`cargo check --workspace` does not work.** Kernel and apps need different
  link args (`-pie -C relocation-model=pic` for apps). Build per-crate with
  the `just` recipes.
- **Adding or reordering a syscall is an ABI break.** Append-only to
  `SyscallTable` in `userlib_sys/src/lib.rs`, bump `SYS_CALL_TABLE_COUNT`,
  run `just cbindgen`, rebuild every app in `user_apps/`.
- **Pure logic shared by kernel and apps goes in `userlib_sys`** so it can be
  unit-tested on the host. Never add a hardware or `embassy-*` dependency to
  `userlib_sys`.

## Style

- Edition 2024, `no_std` everywhere except build scripts.
- Logging is optional: `#[cfg(feature = "defmt")] defmt::info!(...)`, with the
  logged local prefixed `_` so it does not warn when the feature is off.
- Compile-time ABI guards: `const _: () = assert!(...)` and
  `const _: SyscallType = kernel_fn;`.
- `// SAFETY:` comment above every `unsafe` block.

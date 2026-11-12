# Plan 001: Establish a verification baseline and correct repo instructions

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat b619dfa..HEAD -- userlib_sys CRUSH.md justfile rust-toolchain.toml`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: dx
- **Planned at**: commit `b619dfa`

## Why this matters

This repo is a bare-metal Rust OS for the Clockwork PicoCalc (RP2350,
`thumbv8m.main-none-eabihf`). It currently has **zero automated tests** and no
CI, and `cargo check --workspace` does not work: the kernel and the user apps
need different link arguments, so the only verification is "it built". Every
later plan in this directory needs a gate it can actually run, and several of
them need one place to put pure-logic unit tests. `CRUSH.md` — the file an
agent reads for build commands — currently documents a **Go** project
(`make test`, `golangci-lint`, `goimports`), which actively misleads any
executor. Fixing the baseline first makes plans 002–009 verifiable.

## Current state

- `CRUSH.md` — repo-root agent instructions. Entirely wrong; describes Go
  tooling (`go test`, `golangci-lint run --fix`, `goimports -w .`) and refers
  to `.cursor/rules/` and `.github/copilot-instructions.md`, neither of which
  exists in this repo.
- `justfile` — the real build entry points. Verified working recipes:

```make
target := "thumbv8m.main-none-eabihf"

kernel-release board:
    cargo build --bin kernel --release --no-default-features --features {{board}}
    elf2uf2-rs -d target/{{target}}/release/kernel

binary-args := "RUSTFLAGS=\"-C link-arg=-pie -C relocation-model=pic\""

userapp app:
     {{binary-args}} cargo build --bin {{app}} --profile release-binary

userapps: cbindgen
    just userapp calculator
    just userapp snake
    just userapp gallery
    just userapp gif
    just userapp wav_player

cbindgen:
    cbindgen userlib_sys --output userlib_sys.h -q
```

- `rust-toolchain.toml` — pins the toolchain but lists the **wrong** target:

```toml
[toolchain]
channel = "nightly-2025-09-21"
components = ["rust-src", "rustfmt", "rust-analyzer"]
targets = ["thumbv6m-none-eabi"]
```

  `.cargo/config.toml` sets `build.target = "thumbv8m.main-none-eabihf"`, so a
  fresh clone cannot build until that target is installed by hand.

- `userlib_sys/src/lib.rs:1` — `#![no_std]`. This crate is the ABI definition
  shared by the kernel and every user app. Its dependencies (`strum`,
  `bitflags`, `embedded-graphics`) are all target-agnostic, so it is the one
  crate in the workspace that **can** compile and run tests on the host. It is
  where plans 002 and 003 will put shared validation logic.

- Workspace members (root `Cargo.toml`): `kernel`, `userlib_sys`, `userlib`,
  `selection_ui`, `user_apps/{calculator,snake,gallery,gif,wav_player}`.

### Conventions to match

- Rust edition 2024, `no_std` everywhere except build scripts.
- Features gate hardware and logging: `defmt` is optional, and every log site
  is written as `#[cfg(feature = "defmt")] defmt::info!(...)` with the
  logged local prefixed by `_` so it does not warn when the feature is off.
  See `kernel/src/syscalls.rs:63-74` for the exemplar.
- Compile-time ABI assertions use the `const _: () = assert!(...)` form —
  see `userlib_sys/src/lib.rs:16` and `kernel/src/audio.rs:18`.

## Commands you will need

| Purpose          | Command                                                                 | Expected on success |
|------------------|-------------------------------------------------------------------------|---------------------|
| Toolchain target | `rustup target add thumbv8m.main-none-eabihf`                           | exit 0              |
| Kernel builds    | `cargo build --bin kernel --release --no-default-features --features rp235xa` | exit 0        |
| Apps build       | `just userapps`                                                         | exit 0              |
| Host tests       | `cargo test -p userlib_sys --target x86_64-unknown-linux-gnu`           | exit 0, tests pass  |
| Format           | `cargo fmt --check`                                                     | exit 0              |
| Lint (kernel)    | `cargo clippy --bin kernel --no-default-features --features rp235xa -- -D warnings` | exit 0  |

Note: the first network-free `cargo check` attempt fails with
`error: failed to download tinybmp v0.6.0` if the registry cache is cold —
these commands require network access on first run. That is expected.

## Scope

**In scope**:
- `CRUSH.md` (rewrite)
- `AGENTS.md` (create)
- `justfile` (add recipes only)
- `rust-toolchain.toml` (fix the `targets` list)
- `userlib_sys/src/lib.rs` (make host-testable; add the first test)

**Out of scope** (do NOT touch):
- Any file under `kernel/src/`, `userlib/src/`, `user_apps/` — later plans own
  those, and changing them here makes their drift checks fail.
- `.cargo/config.toml` — the `build.target` default is deliberate; changing it
  breaks every `just` recipe.
- `picolibc/` — a git submodule, built only by `just newlib`.

## Git workflow

- Branch: `advisor/001-verification-baseline`
- Commit per step. Message style matches `git log` (short, imperative,
  lowercase — e.g. `small fixes`, `validate elf header`).
- Do NOT push or open a PR unless the operator asks.

## Steps

### Step 1: Fix the toolchain target list

In `rust-toolchain.toml`, replace the `targets` array with the target the repo
actually builds for:

```toml
targets = ["thumbv8m.main-none-eabihf"]
```

**Verify**: `rustup target list --installed | grep thumbv8m.main-none-eabihf`
→ prints the target.

### Step 2: Make `userlib_sys` host-testable

`userlib_sys` is `#![no_std]`, which means `cargo test` on the host cannot link
a test harness. Change line 1 of `userlib_sys/src/lib.rs` from:

```rust
#![no_std]
```

to:

```rust
#![cfg_attr(not(test), no_std)]
```

Then add a module at the end of the file that proves the harness works and
locks in the existing ABI invariant:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use strum::EnumCount;

    #[test]
    fn syscall_table_count_matches_enum() {
        assert_eq!(SYS_CALL_TABLE_COUNT, SyscallTable::COUNT);
    }
}
```

Do not change any other line of the file. In particular leave the existing
compile-time guard at `userlib_sys/src/lib.rs:16` in place:

```rust
pub const SYS_CALL_TABLE_COUNT: usize = 17;
const _: () = assert!(SYS_CALL_TABLE_COUNT == SyscallTable::COUNT);
```

**Verify**: `cargo test -p userlib_sys --target x86_64-unknown-linux-gnu`
→ exit 0, `1 passed`.

**Verify (no regression on target)**: `just userapp calculator` → exit 0.

### Step 3: Add `just` verification recipes

Append to `justfile` (do not modify existing recipes):

```make
# host-side unit tests (userlib_sys is the only target-agnostic crate)
test:
    cargo test -p userlib_sys --target x86_64-unknown-linux-gnu

# everything a change must pass before review
check: test
    cargo fmt --check
    cargo build --bin kernel --release --no-default-features --features rp235xa
    just userapps
```

**Verify**: `just test` → exit 0. `just check` → exit 0.

### Step 4: Replace `CRUSH.md` and add `AGENTS.md`

Delete the Go content in `CRUSH.md` and write the real thing. `AGENTS.md` is
the file agents look for by convention; make `CRUSH.md` a short pointer to it
so the two cannot drift apart.

`AGENTS.md` must contain, in this order:

1. **What this is**: bare-metal Rust OS for the Clockwork PicoCalc, RP2350
   (Cortex-M33, `thumbv8m.main-none-eabihf`), kernel on core0 + a single
   dynamically loaded user app on core1.
2. **Layout**: one line per workspace member (`kernel`, `userlib_sys` = C ABI
   syscall table, `userlib` = safe Rust wrapper, `selection_ui`, `user_apps/*`).
3. **Commands**: the table from "Commands you will need" above, plus
   `just check`, `just test`, `just userapps`, `just kernel-release rp235xa`,
   `just copy-userapps`.
4. **Hard rules**:
   - User apps are untrusted input. Every pointer, length and path that
     crosses a syscall must be validated kernel-side; a syscall must never
     panic on bad arguments.
   - `cargo check --workspace` does not work (kernel and apps need different
     link args). Build per-crate with the `just` recipes.
   - Adding or reordering a syscall is an ABI break: bump
     `SYS_CALL_TABLE_COUNT` in `userlib_sys/src/lib.rs`, rerun
     `just cbindgen`, and rebuild every app in `user_apps/`.
   - Pure logic that both the kernel and apps need goes in `userlib_sys` so it
     can be unit-tested on the host.
5. **Style**: edition 2024, `no_std`, `#[cfg(feature = "defmt")]` around every
   log with `_`-prefixed locals, `const _: () = assert!(...)` for ABI guards.

`CRUSH.md` becomes a two-line pointer to `AGENTS.md`.

**Verify**: `grep -ci "golangci\|goimports\|go test" CRUSH.md` → `0`.
`test -f AGENTS.md && grep -c "thumbv8m.main-none-eabihf" AGENTS.md` → ≥ 1.

## Test plan

- New test: `userlib_sys/src/lib.rs` `tests::syscall_table_count_matches_enum`
  — proves the host harness runs and pins the syscall-count invariant.
- There is no existing test to model on; this is the first one in the repo.
- Verification: `just test` → exit 0, 1 test passed.

## Done criteria

ALL must hold:

- [ ] `just test` exits 0 with at least 1 passing test
- [ ] `just check` exits 0
- [ ] `grep -ci "golangci\|goimports\|go test" CRUSH.md` returns 0
- [ ] `AGENTS.md` exists and documents the four hard rules from step 4
- [ ] `rust-toolchain.toml` lists `thumbv8m.main-none-eabihf`
- [ ] `git status` shows no modified files outside the in-scope list
- [ ] `plans/README.md` status row updated

## STOP conditions

- `cargo test -p userlib_sys --target x86_64-unknown-linux-gnu` fails to link
  or compile for a reason other than a missing `std` attribute — report the
  exact error instead of adding dependencies or feature flags to make it pass.
- `just userapps` fails after step 2 — the `cfg_attr` change must be
  invisible to target builds; if it is not, revert step 2 and report.
- Any build requires adding a new dependency. Do not add one.

## Maintenance notes

- `just check` is the gate every later plan (002–009) references. If a recipe
  is renamed, those plans must be updated.
- `userlib_sys` must stay target-agnostic — never add a hardware or
  `embassy-*` dependency to it, or host tests die and plans 002/003 lose their
  verification story.
- Deferred on purpose: CI config, and host tests for the kernel (impossible
  while `kernel` depends on `embassy-rp`, which is ARM-only).

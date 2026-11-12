# Plan 002: Make filesystem and buffer syscalls reject bad arguments instead of panicking

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat b619dfa..HEAD -- kernel/src/syscalls.rs userlib_sys/src/lib.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: plans/001-verification-baseline.md
- **Category**: bug
- **Planned at**: commit `b619dfa`

## Why this matters

This is a bare-metal OS for the Clockwork PicoCalc (RP2350). The kernel runs on
core0; a dynamically loaded, **untrusted** user app runs on core1 and calls the
kernel through a table of `extern "C"` function pointers. A user app that
passes a bad path, a null pointer or a zero length currently takes the whole
device down: the syscall implementations `unwrap()` and `expect()` on
user-controlled values, and a kernel panic on the PicoCalc means a hard reset
(the watchdog in `kernel/src/main.rs:106-123` has a 3 s timeout and the panic
handler is `panic-probe`). Every one of these is reachable from safe user code
— `userlib::fs::read_file("", 0, &mut buf)` is enough. The README states the
current focus is "making sure the kernel is as stable as possible"; these are
the cheapest, highest-frequency crashes to remove.

## Current state

`kernel/src/syscalls.rs` is the kernel side of the ABI. Each syscall is an
`extern "C" fn` preceded by a compile-time type check against the signature
declared in `userlib_sys`, e.g.:

```rust
const _: ReadFile = read_file;
pub extern "C" fn read_file(...)
```

Keep that pattern. The problems, all verified at these lines:

**1. `try_lock().expect(...)` — panics on contention (4 sites)**

`kernel/src/syscalls.rs:236`, `:311`, `:350`, `:371`:

```rust
let mut guard = SDCARD.get().try_lock().expect("Failed to get sdcard");
let sd = guard.as_mut().unwrap();
```

`SDCARD` is `LazyLock<Mutex<CriticalSectionRawMutex, Option<SdCard>>>`
(`kernel/src/storage.rs:27`). Core0 holds this same lock in
`prog_search_handler` (`kernel/src/main.rs:411`, every 5 s), in the ELF loader
(`kernel/src/elf.rs:35`) and in the USB MTP path
(`kernel/src/mtp/mod.rs:252`, `:284`). A user app doing file I/O while core0
polls the card panics the kernel. The `.as_mut().unwrap()` on the next line is
a second panic if the card was never initialized.

**2. Unchecked `open_dir` on a user-supplied path component (2 sites)**

`kernel/src/syscalls.rs:218` (in `recurse_dir`) and `:281` (in `recurse_file`):

```rust
let dir = dir.open_dir(dirs[0]).unwrap();
```

Any path naming a directory that does not exist panics the kernel.

**3. Unchecked file I/O results**

`kernel/src/syscalls.rs:317` `file.read(buf).unwrap()`, `:355`
`file.seek_from_start(start_from as u32).unwrap()`, `:356`
`file.write(buf).unwrap()`, and `:204` `.unwrap()` on `iterate_dir_lfn`.
A short read, a seek past EOF, or a full card panics the kernel.

**4. `max_str_len - 1` underflows**

`kernel/src/syscalls.rs:180-188`:

```rust
unsafe fn copy_entry_to_user_buf(name: &[u8], dest: *mut c_char, max_str_len: usize) {
    if !dest.is_null() {
        let len = name.len().min(max_str_len - 1);
```

`max_str_len` comes straight from the user (`list_dir`'s last argument). A
caller passing `0` underflows to `usize::MAX`, so `min` returns `name.len()`
and the write runs past the user's buffer.

**5. `list_dir` ignores its own bounds and indexes `dirs[0]` blindly**

`kernel/src/syscalls.rs:223-251`. It builds `files` from the raw
`entries`/`files_len` pair with no null or zero check, then reads `dirs[0]`
(`:240`) — `str::split` always yields at least one element so that specific
index is safe, but the surrounding logic silently writes nothing for any path
not starting with `/`, which the caller cannot distinguish from "empty
directory".

**6. `get_ms` panics before the first app launch**

`kernel/src/syscalls.rs:86-94`:

```rust
pub static mut MS_SINCE_LAUNCH: Option<Instant> = None;

const _: GetMs = get_ms;
pub extern "C" fn get_ms() -> u64 {
    Instant::now()
        .duration_since(unsafe { MS_SINCE_LAUNCH.unwrap() })
        .as_millis()
}
```

It is set in `kernel/src/main.rs:225` just before `entry()`, so in practice it
is `Some` — but the `unwrap` is free to remove.

**7. `send_audio_buffer` spins forever**

`kernel/src/syscalls.rs:393-412`:

```rust
pub extern "C" fn send_audio_buffer(ptr: *const u8, len: usize) {
    let buf = unsafe { core::slice::from_raw_parts(ptr, len) };

    while !AUDIO_BUFFER_READY.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }

    if buf.len() == AUDIO_BUFFER_SAMPLES * 2 {
```

The wrong-size check happens **after** the unbounded spin. If the audio task on
core0 is not draining (e.g. the app called this before any audio was
configured), core1 spins forever with no timeout — the app hangs and the only
way out is a reset. Note the length check itself is correct and must stay.

### Conventions to match

- Error reporting at the ABI boundary is by **return value**, not panic. The
  existing exemplar is `read_file`, which returns the number of bytes read and
  `0` on failure (`kernel/src/syscalls.rs:283-324`), and `list_dir`, which
  returns the number of entries written.
- Logging: `#[cfg(feature = "defmt")] defmt::warn!(...)` with `_`-prefixed
  locals. Exemplar at `kernel/src/syscalls.rs:63-74` and `:406-411`.
- Safety comments: every `unsafe` block gets a `// SAFETY:` line. Exemplar at
  `kernel/src/syscalls.rs:64-66`.
- Compile-time ABI guards use `const _: Type = func;` — already present on
  every syscall; do not remove them.

## Commands you will need

| Purpose       | Command                                                                 | Expected on success |
|---------------|-------------------------------------------------------------------------|---------------------|
| Host tests    | `just test`                                                             | exit 0, tests pass  |
| Kernel builds | `cargo build --bin kernel --release --no-default-features --features rp235xa` | exit 0        |
| Apps build    | `just userapps`                                                         | exit 0              |
| Full gate     | `just check`                                                            | exit 0              |
| Format        | `cargo fmt --check`                                                     | exit 0              |

(`just test` and `just check` are added by plan 001 — run that plan first.)

## Scope

**In scope**:
- `kernel/src/syscalls.rs`
- `userlib_sys/src/lib.rs` — only to add the shared path-splitting helper and
  its host tests described in step 1.

**Out of scope** (do NOT touch, even though they look related):
- `kernel/src/elf.rs` — the loader has its own panics; plan 003 owns it.
  Changing it here collides with that plan.
- `kernel/src/storage.rs` — the `SdCard` API stays as-is; this plan only
  changes how syscalls *call* it.
- `kernel/src/framebuffer.rs` and the drawing syscalls (`draw_iter`,
  `fill_rect`, `blit`) — plan 004 owns pixel-buffer validation.
- `userlib/src/lib.rs` and anything under `user_apps/` — the user-side API
  does not change in this plan. Every change here must be
  backward-compatible: same syscall order, same signatures, same
  `SYS_CALL_TABLE_COUNT`.

## Git workflow

- Branch: `advisor/002-syscall-arg-validation`
- Commit per step; short imperative lowercase messages matching `git log`.
- Do NOT push or open a PR unless the operator asks.

## Steps

### Step 1: Add a tested path splitter to `userlib_sys`

Both `read_file`/`write_file` (fixed `[&str; 8]` array, `kernel/src/syscalls.rs:296-305`
and `:338-347`) and `file_len`/`list_dir` (heap `Vec<&str>`, `:367` and `:234`)
re-implement path splitting differently. Put one correct version in
`userlib_sys` so it can be unit-tested on the host.

Add to `userlib_sys/src/lib.rs`:

```rust
/// Maximum number of path components a syscall will walk.
pub const MAX_PATH_COMPONENTS: usize = 8;

/// Splits an absolute syscall path into its non-empty components.
///
/// Returns `None` for paths that are not absolute, are empty, contain an
/// empty component, contain a `.` or `..` component, or have more than
/// [`MAX_PATH_COMPONENTS`] components. Callers must treat `None` as
/// "reject this request".
pub fn split_path(path: &str) -> Option<([&str; MAX_PATH_COMPONENTS], usize)> {
    let rest = path.strip_prefix('/')?;
    if rest.is_empty() {
        return Some(([""; MAX_PATH_COMPONENTS], 0));
    }

    let mut out = [""; MAX_PATH_COMPONENTS];
    let mut count = 0;
    for part in rest.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return None;
        }
        if count == MAX_PATH_COMPONENTS {
            return None;
        }
        out[count] = part;
        count += 1;
    }
    Some((out, count))
}
```

Rejecting `..` matters: `recurse_dir`/`recurse_file` walk components
blindly, so a path like `/gifs/../secret` would otherwise be handed to
`embedded_sdmmc` as-is.

Add host tests in the `mod tests` block created by plan 001:

```rust
#[test]
fn split_path_accepts_root() {
    let (_, count) = split_path("/").unwrap();
    assert_eq!(count, 0);
}

#[test]
fn split_path_splits_components() {
    let (parts, count) = split_path("/gifs/a.gif").unwrap();
    assert_eq!(count, 2);
    assert_eq!(&parts[..count], &["gifs", "a.gif"]);
}

#[test]
fn split_path_rejects_relative_path() {
    assert!(split_path("gifs/a.gif").is_none());
}

#[test]
fn split_path_rejects_empty_input() {
    assert!(split_path("").is_none());
}

#[test]
fn split_path_rejects_double_slash() {
    assert!(split_path("/gifs//a.gif").is_none());
}

#[test]
fn split_path_rejects_parent_component() {
    assert!(split_path("/gifs/../secret").is_none());
}

#[test]
fn split_path_rejects_too_many_components() {
    assert!(split_path("/a/b/c/d/e/f/g/h/i").is_none());
}
```

**Verify**: `just test` → exit 0, 7 new tests pass.

### Step 2: Replace every `try_lock().expect()` with a graceful bail-out

At `kernel/src/syscalls.rs:236`, `:311`, `:350`, `:371`, replace the two-line
lock-and-unwrap with a pattern that returns the function's "nothing happened"
value. For the three functions returning `usize` (`list_dir`, `read_file`,
`file_len`) that value is `0`; `write_file` returns `()`.

Shape to produce (shown for `read_file`):

```rust
let Some(mut guard) = SDCARD.get().try_lock().ok() else {
    #[cfg(feature = "defmt")]
    defmt::warn!("read_file: sdcard busy");
    return 0;
};
let Some(sd) = guard.as_mut() else {
    #[cfg(feature = "defmt")]
    defmt::warn!("read_file: sdcard not initialized");
    return 0;
};
```

Do not change `try_lock` to a blocking `lock` — these are synchronous
`extern "C"` functions called from core1 and cannot await.

**Verify**: `grep -c 'try_lock().expect' kernel/src/syscalls.rs` → `0`.
`cargo build --bin kernel --release --no-default-features --features rp235xa`
→ exit 0.

### Step 3: Make directory and file walking fallible

Change `recurse_dir` (`kernel/src/syscalls.rs:208-220`) and `recurse_file`
(`:253-282`) so a missing directory returns an error instead of panicking:

- In `recurse_dir`, replace `dir.open_dir(dirs[0]).unwrap()` with a `match` /
  `let ... else` that returns `0` written entries.
- In `recurse_file`, replace `dir.open_dir(dirs[0]).unwrap()` with
  `.map_err(|_| ())?` — the function already returns `Result<T, ()>`.
- Replace `.expect("Failed to iterate dir")` at `:267-271` with
  `.map_err(|_| ())?`.
- In `get_dir_entries` (`:187-206`), replace the trailing `.unwrap()` on
  `iterate_dir_lfn` with an `if let Err(_)` that returns the count written so
  far.

Then fix the three unchecked I/O calls:
- `:317` `file.read(buf).unwrap()` → `file.read(buf).unwrap_or(0)`
- `:355` `file.seek_from_start(start_from as u32).unwrap()` → on error, skip
  the write and return without writing
- `:356` `file.write(buf).unwrap()` → ignore the error after a
  `#[cfg(feature = "defmt")] defmt::warn!` (`write_file` returns `()`, so
  there is no channel to report it; note this in the maintenance section)

**Verify**: `grep -n 'unwrap()\|expect(' kernel/src/syscalls.rs` → every
remaining hit is inside the drawing syscalls (`draw_iter`, `fill_rect`,
`blit`, lines ~98-146) or `MS_SINCE_LAUNCH`; no hit in the filesystem
functions. `just check` → exit 0.

### Step 4: Harden `copy_entry_to_user_buf` and `list_dir`

In `copy_entry_to_user_buf` (`kernel/src/syscalls.rs:180-188`), reject the
degenerate size before doing arithmetic:

```rust
unsafe fn copy_entry_to_user_buf(name: &[u8], dest: *mut c_char, max_str_len: usize) {
    if dest.is_null() || max_str_len == 0 {
        return;
    }
    let len = name.len().min(max_str_len - 1);
    // SAFETY: caller guarantees `dest` is valid for `max_str_len` bytes;
    // `len` is at most `max_str_len - 1`, leaving room for the terminator.
    unsafe {
        ptr::copy_nonoverlapping(name.as_ptr(), dest, len);
        *dest.add(len) = 0;
    }
}
```

In `list_dir` (`:223-251`):
- Return `0` immediately if `entries.is_null()`, `files_len == 0`,
  `max_entry_str_len == 0`, `dir.is_null()` or `len == 0`.
- Replace the hand-rolled `dir.split('/')` + `dirs[0].is_empty()` logic with
  `userlib_sys::split_path`. On `None`, log a `defmt::warn!` and return `0`.
  Component count `0` means the root directory (`get_dir_entries(&root, ...)`);
  otherwise recurse with the component slice.

Apply the same `split_path` treatment to `read_file` (`:283-324`),
`write_file` (`:325-362`) and `file_len` (`:363-382`), deleting their
hand-rolled component arrays and the `Vec<&str>` allocation in `file_len`.
Each rejects a `None` result by returning its "nothing happened" value.

**Verify**: `grep -c "split('/')" kernel/src/syscalls.rs` → `0`.
`grep -c 'split_path' kernel/src/syscalls.rs` → `4`. `just check` → exit 0.

### Step 5: Remove the two remaining hang/panic primitives

`get_ms` (`kernel/src/syscalls.rs:86-94`): return `0` instead of panicking
when `MS_SINCE_LAUNCH` is `None`:

```rust
const _: GetMs = get_ms;
pub extern "C" fn get_ms() -> u64 {
    // SAFETY: only written by `userland_task` on core1 before `entry()` is
    // called, and read-only here.
    let Some(start) = unsafe { MS_SINCE_LAUNCH } else {
        return 0;
    };
    Instant::now().duration_since(start).as_millis()
}
```

`send_audio_buffer` (`:393-412`): move the size check **before** the spin, and
bound the spin with a deadline so a stalled audio task cannot hang the app:

```rust
const _: SendAudioBuffer = send_audio_buffer;
pub extern "C" fn send_audio_buffer(ptr: *const u8, len: usize) {
    if ptr.is_null() || len != AUDIO_BUFFER_SAMPLES * 2 {
        #[cfg(feature = "defmt")]
        defmt::warn!(
            "user audio stream was wrong size: {} should be {}",
            len,
            AUDIO_BUFFER_SAMPLES * 2
        );
        return;
    }

    // SAFETY: caller guarantees `ptr` is valid for `len` bytes
    let buf = unsafe { core::slice::from_raw_parts(ptr, len) };

    // Bounded wait: a stalled `audio_handler` must not hang core1 forever.
    let deadline = Instant::now() + Duration::from_millis(250);
    while !AUDIO_BUFFER_READY.load(Ordering::Acquire) {
        if Instant::now() > deadline {
            #[cfg(feature = "defmt")]
            defmt::warn!("send_audio_buffer: timed out waiting for audio buffer");
            return;
        }
        core::hint::spin_loop();
    }

    AUDIO_BUFFER_READY.store(false, Ordering::Release);
    // SAFETY: `AUDIO_BUFFER_READY` was observed true and is now false, so
    // `audio_handler` is not touching `AUDIO_BUFFER`. Length checked above.
    unsafe { AUDIO_BUFFER.copy_from_slice(buf) };
    AUDIO_BUFFER_WRITTEN.store(true, Ordering::Release);
}
```

`Duration` needs adding to the existing `embassy_time` import at
`kernel/src/syscalls.rs:4`.

**Verify**: `cargo build --bin kernel --release --no-default-features --features rp235xa`
→ exit 0. `just check` → exit 0.

### Step 6: Confirm the ABI did not move

The whole point is that existing app binaries keep working.

**Verify**: `grep -n 'SYS_CALL_TABLE_COUNT: usize' userlib_sys/src/lib.rs` →
still `17`. `just test` → exit 0 (the count test from plan 001 passes).
`just userapps` → exit 0.

## Test plan

- New tests: the seven `split_path` cases in step 1, in the `mod tests` block
  in `userlib_sys/src/lib.rs`. They cover root, normal split, relative path,
  empty input, double slash, `..` traversal, and component overflow.
- No existing test to model on beyond the one plan 001 adds in the same block.
- The kernel-side changes cannot be unit-tested (they need `embassy-rp` and
  real hardware); their gate is "compiles and the ABI count is unchanged",
  plus the manual device check in "Done criteria".
- Verification: `just test` → exit 0, 8 tests total passing (1 from plan 001 +
  7 new).

## Done criteria

ALL must hold:

- [ ] `just test` exits 0; the 7 `split_path` tests exist and pass
- [ ] `just check` exits 0
- [ ] `grep -c 'try_lock().expect' kernel/src/syscalls.rs` → 0
- [ ] `grep -c "split('/')" kernel/src/syscalls.rs` → 0
- [ ] `grep -c 'split_path' kernel/src/syscalls.rs` → 4
- [ ] No `unwrap()`/`expect()` remains in `list_dir`, `read_file`,
      `write_file`, `file_len`, `recurse_dir`, `recurse_file`,
      `get_dir_entries`, `copy_entry_to_user_buf`, `get_ms`,
      `send_audio_buffer`
- [ ] `SYS_CALL_TABLE_COUNT` is still `17`
- [ ] `git status` shows only `kernel/src/syscalls.rs` and
      `userlib_sys/src/lib.rs` modified
- [ ] `plans/README.md` status row updated

**Manual device check** (for the operator, not the executor — record that it
is pending if no hardware is available): flash with
`just kernel-release rp235xa`, copy the apps with `just copy-userapps`, then
launch the gif app and press Esc during playback. Expected: no reset, kernel UI
returns.

## STOP conditions

- `embedded_sdmmc`'s `Directory::open_dir` or `File::read` turn out to have
  different error types than `Result<_, _>` — report the signature instead of
  guessing.
- Removing a `.unwrap()` requires changing a syscall's return type or
  signature. That is an ABI break and is out of scope for this plan — stop and
  report.
- `just userapps` fails after any step: the app-facing ABI must not change.
- You find that `MS_SINCE_LAUNCH` is written from somewhere other than
  `kernel/src/main.rs:225` — the `SAFETY` comment in step 5 would then be
  wrong.

## Maintenance notes

- `write_file` still has no way to report failure to the caller; it returns
  `()`. Changing it to return a written-byte count is a deliberate follow-up
  (it is an ABI break, so it belongs with the next `SYS_CALL_TABLE_COUNT`
  bump — see plan 005).
- The `try_lock` bail-outs mean a user app can now observe a *transient*
  `0`-byte read when core0 happens to hold the SD lock (it polls every 5 s in
  `prog_search_handler`). That is a behavior change user apps must tolerate;
  plan 005 addresses it properly by distinguishing "busy" from "not found" in
  the return value.
- Reviewer should scrutinize: that every early return uses the function's
  documented "nothing happened" value, and that no `SAFETY` comment was
  dropped while editing the `unsafe` blocks.
- `split_path` rejects `..`, so any future feature that legitimately needs
  relative paths must extend it deliberately rather than bypass it.

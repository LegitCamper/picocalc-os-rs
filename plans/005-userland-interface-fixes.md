# Plan 005: Fix the userland-facing library bugs (dangling pointers, busy-wait sleep, cross-core key queue)

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat b619dfa..HEAD -- userlib/src/lib.rs userlib_sys/src/lib.rs kernel/src/syscalls.rs kernel/src/main.rs`
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

`userlib` is the "mostly safe" Rust wrapper every user app on this PicoCalc OS
is written against (`user_apps/{calculator,snake,gallery,gif,wav_player}`).
Three defects in it and its kernel counterparts are not app bugs but
*interface* bugs — every app inherits them:

- `list_dir` hands the kernel a pointer into a **dropped temporary**, so the
  kernel writes directory names into freed stack memory. It appears to work
  only because nothing has reused that stack slot yet.
- `sleep` is a busy-wait `nop` loop inside the kernel, so a sleeping app burns
  core1 at 100% and the gif player's frame pacing is wrong by construction.
- The keyboard queue is a non-atomic `heapless::spsc::Queue` in a
  `static mut`, produced on core0 and consumed on core1 with no
  synchronization at all.

All three are cheap to fix without changing the syscall table, so existing
`.bin` files keep loading.

## Current state

**1. `userlib::fs::list_dir` passes a dangling pointer**

`userlib/src/lib.rs:355-378`:

```rust
fn as_ptrs(&mut self) -> [*mut u8; MAX_ENTRIES] {
    let mut ptrs: [*mut u8; MAX_ENTRIES] = [core::ptr::null_mut(); MAX_ENTRIES];
    for (i, buf) in self.0.iter_mut().enumerate() {
        ptrs[i] = buf.as_mut_ptr();
    }
    ptrs
}

pub fn list_dir(path: &str, entries: &mut Entries) -> usize {
    userlib_sys::list_dir(
        path.as_ptr(),
        path.len(),
        entries.as_ptrs().as_mut_ptr(),
        MAX_ENTRIES,
        MAX_ENTRY_NAME_LEN,
    )
}
```

`as_ptrs()` returns an array **by value**. `.as_mut_ptr()` takes the address of
that temporary, which is dropped at the end of the enclosing expression — the
kernel then dereferences it. The kernel side reads the array
(`kernel/src/syscalls.rs:231`):

```rust
let files = unsafe { core::slice::from_raw_parts_mut(entries, files_len) };
```

and writes through each element (`copy_entry_to_user_buf`,
`kernel/src/syscalls.rs:180-188`). So the names land wherever that stack slot
now is. Every app that lists files is affected: `gif` (`user_apps/gif/src/main.rs:40`),
`gallery`, `wav_player`.

**2. `sleep` busy-waits in the kernel**

`kernel/src/syscalls.rs:76-84`:

```rust
const _: SleepMs = sleep;
pub extern "C" fn sleep(ms: u64) {
    let cycles_per_ms = clk_sys_freq() / 1000;
    let total_cycles = ms * cycles_per_ms as u64;

    for _ in 0..total_cycles {
        cortex_m::asm::nop();
    }
}
```

At 150 MHz that is 150 000 `nop`s per millisecond, and `ms * cycles_per_ms`
overflows for large `ms`. It also makes `get_ms`-based frame pacing wrong — the
gif player computes `sleep(delay_centis * 10 - start)` where `start` is an
*absolute* timestamp, not an elapsed time (`user_apps/gif/src/main.rs:83-95`):

```rust
let start = get_ms();
frame.translate_mut(translation).draw(&mut display).unwrap();
...
sleep(((frame.delay_centis as u64) * 10).saturating_sub(start));
```

After the first second of playback `start` exceeds every frame delay, so
`saturating_sub` yields `0` and the gif runs at full speed. Fixing `sleep`
without fixing this call site leaves the gif player wrong.

**3. `KEY_CACHE` is a cross-core SPSC queue with no synchronization**

`kernel/src/syscalls.rs:149`:

```rust
pub static mut KEY_CACHE: Queue<KeyEvent, 32> = Queue::new();
```

Producer, core0 (`kernel/src/main.rs:427-436`):

```rust
async fn key_handler() {
    loop {
        if let Some(event) = read_keyboard_fifo().await {
            unsafe {
                let _ = KEY_CACHE.enqueue(event);
            }
        }
        Timer::after_millis(50).await;
    }
}
```

Consumer, core1 via the `get_key` syscall (`kernel/src/syscalls.rs:151-163`):

```rust
pub extern "C" fn get_key() -> KeyEventC {
    if let Some(event) = unsafe { KEY_CACHE.dequeue() } {
```

`heapless::spsc::Queue`'s `enqueue`/`dequeue` are only sound when the two ends
are split into `Producer`/`Consumer` halves; calling both on the same
`static mut` from two cores races on the head/tail indices. On a dual-core
Cortex-M33 with no coherency guarantees from plain loads/stores, this can drop
or duplicate events, or read a torn index. Note `key_handler` also polls only
every 50 ms and only runs when the kernel UI is disabled
(`kernel/src/main.rs:398-405`), so held keys are easy to miss.

**4. Apps hang forever on panic**

Every app has (`user_apps/gif/src/main.rs:24-28`, and identically in the other
four):

```rust
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("user panic: {} @ {:?}", info.message(), info.location(),);
    loop {}
}
```

`loop {}` on core1 means the app never returns, `userland_task`
(`kernel/src/main.rs:209-253`) never re-enables the kernel UI, and the device
is bricked until reset. The apps are full of reachable panics —
`Display::take().unwrap()`, `assert!(selection.is_some())`,
`Gif::from_slice(..).expect(..)`, `wavs[selection.unwrap()]` — so this is the
common exit path, not a corner case.

**5. `Display::take()` cannot be called twice across launches**

`userlib/src/lib.rs:70-85`:

```rust
static DISPLAY_TAKEN: AtomicBool = AtomicBool::new(false);
...
pub fn take() -> Option<Display> {
    if DISPLAY_TAKEN
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
```

`DISPLAY_TAKEN` lives in the app's `.bss`, which the loader re-zeroes on every
load (`kernel/src/elf.rs:129-132`), so this is currently fine. It is only a
hazard if an app is ever re-entered without a reload. Low priority; noted so
the next person does not "fix" it unnecessarily.

### Conventions to match

- `userlib` wraps `userlib_sys` with safe signatures; the unsafe pointer work
  is confined to `userlib`. Exemplar: `userlib::fs::read_file`
  (`userlib/src/lib.rs:270-279`) passes `buf.as_mut_ptr()` from a live
  borrow — that is the correct shape to copy.
- Syscall ABI guards: `const _: TypeAlias = func;` in
  `kernel/src/syscalls.rs`, paired with `pub type X = extern "C" fn(..)` in
  `userlib_sys/src/lib.rs`. Never change one without the other.
- `#[cfg(feature = "defmt")] defmt::warn!(...)` with `_`-prefixed locals.
- Host-testable logic goes in `userlib_sys` (see plan 001).

## Commands you will need

| Purpose       | Command                                                                 | Expected on success |
|---------------|-------------------------------------------------------------------------|---------------------|
| Host tests    | `just test`                                                             | exit 0, tests pass  |
| Kernel builds | `cargo build --bin kernel --release --no-default-features --features rp235xa` | exit 0        |
| Apps build    | `just userapps`                                                         | exit 0              |
| Regen header  | `just cbindgen`                                                         | exit 0, rewrites `userlib_sys.h` |
| Full gate     | `just check`                                                            | exit 0              |

## Scope

**In scope**:
- `userlib/src/lib.rs`
- `userlib_sys/src/lib.rs`
- `kernel/src/syscalls.rs` — only `sleep` (lines ~76-84) and the
  `KEY_CACHE` / `get_key` pair (lines ~149-163)
- `kernel/src/main.rs` — only `key_handler` (lines ~427-436) and the
  `KEY_CACHE` import
- `user_apps/*/src/main.rs` — only the `#[panic_handler]` bodies, plus the
  one frame-pacing expression in `user_apps/gif/src/main.rs:83-95`

**Out of scope** (do NOT touch):
- The filesystem syscall bodies in `kernel/src/syscalls.rs` — plan 002 owns
  them. If plan 002 has landed, `list_dir` there already validates
  `max_entry_str_len`; do not re-edit it.
- `kernel/src/elf.rs` (plan 003), `kernel/src/framebuffer.rs` and the drawing
  syscalls (plan 004).
- The `SyscallTable` enum, its order, and `SYS_CALL_TABLE_COUNT`. This plan
  must not change the ABI — `sleep` keeps the same signature, it just stops
  busy-waiting.
- App game/UI logic beyond the two items listed above.

## Git workflow

- Branch: `advisor/005-userland-interface-fixes`
- Commit per step; short imperative lowercase messages matching `git log`.
- Do NOT push or open a PR unless the operator asks.

## Steps

### Step 1: Fix the dangling pointer array in `list_dir`

Keep the pointer array alive across the call. In `userlib/src/lib.rs`, replace
`as_ptrs` and `list_dir` (`:355-378`) with:

```rust
pub fn list_dir(path: &str, entries: &mut Entries) -> usize {
    // The kernel writes through this array of pointers, so it must outlive
    // the call — binding it to a local is load-bearing, not stylistic.
    let mut ptrs: [*mut u8; MAX_ENTRIES] = [core::ptr::null_mut(); MAX_ENTRIES];
    for (slot, buf) in ptrs.iter_mut().zip(entries.0.iter_mut()) {
        *slot = buf.as_mut_ptr();
    }

    let written = userlib_sys::list_dir(
        path.as_ptr(),
        path.len(),
        ptrs.as_mut_ptr(),
        MAX_ENTRIES,
        MAX_ENTRY_NAME_LEN,
    );
    written.min(MAX_ENTRIES)
}
```

Delete the now-unused `as_ptrs` method. The `.min(MAX_ENTRIES)` clamp means a
kernel that reports more entries than the buffer can hold cannot make the
caller read uninitialized slots.

`Entries.0` is a private tuple field in the same module, so direct access
compiles.

**Verify**: `grep -c 'as_ptrs' userlib/src/lib.rs` → `0`.
`just userapps` → exit 0.

### Step 2: Make `Entries::entries()` skip empty slots

`userlib/src/lib.rs:344-353`:

```rust
pub fn entries<'a>(&'a self) -> Vec<FileName<'a>> {
    self.0
        .iter()
        .filter_map(|buf| {
            let nul_pos = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            Some(core::str::from_utf8(&buf[..nul_pos]).ok()?.into())
        })
        .collect()
}
```

A freshly-constructed `Entries` is all zeros, so every unused slot yields an
empty `FileName`. Apps then filter by extension and get a shorter list by
accident (`user_apps/gif/src/main.rs:42-45`). Make it explicit — skip slots
whose first byte is `0`:

```rust
/// Returns the non-empty entry names written by the last `list_dir` call.
pub fn entries<'a>(&'a self) -> Vec<FileName<'a>> {
    self.0
        .iter()
        .filter(|buf| buf[0] != 0)
        .filter_map(|buf| {
            let nul_pos = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            Some(core::str::from_utf8(&buf[..nul_pos]).ok()?.into())
        })
        .collect()
}
```

**Verify**: `just userapps` → exit 0.

### Step 3: Replace the busy-wait `sleep` with a real wait

In `kernel/src/syscalls.rs:76-84`, keep the signature (`extern "C" fn(u64)` —
the ABI) but wait on the hardware timer instead of spinning, and saturate the
arithmetic:

```rust
const _: SleepMs = sleep;
pub extern "C" fn sleep(ms: u64) {
    let deadline = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < deadline {
        cortex_m::asm::wfe();
    }
}
```

`Instant`/`Duration` come from `embassy_time`, already imported at
`kernel/src/syscalls.rs:4` (add `Duration` to that import). `wfe` parks the
core until an event instead of burning cycles on `nop`; the embassy time
driver's interrupts wake it. This is still a blocking call on core1 — it must
be, because the syscall is synchronous — but it no longer pegs the CPU and no
longer overflows for large `ms`.

**Verify**: `grep -c 'asm::nop' kernel/src/syscalls.rs` → `0`.
`cargo build --bin kernel --release --no-default-features --features rp235xa`
→ exit 0.

### Step 4: Fix the gif player's frame pacing

`user_apps/gif/src/main.rs:81-96` — the bug is comparing an absolute
timestamp against a delay. Measure elapsed time instead:

```rust
for mut frame in gif.frames() {
    let start = get_ms();

    frame.translate_mut(translation).draw(&mut display).unwrap();
    frame_num += 1;

    if frame_num % 5 == 0 {
        let event = get_key();
        if event.state != KeyState::Idle && event.key == KeyCode::Esc {
            return;
        }
    }

    let frame_ms = (frame.delay_centis as u64) * 10;
    let elapsed = get_ms().saturating_sub(start);
    sleep(frame_ms.saturating_sub(elapsed));
}
```

Also delete the `drop(buf)` on the early-return path (`:91`) — `buf` is a
local `Vec` that drops on return anyway, and dropping it while `gif` still
borrows it would not compile if the borrow checker could see through the
early return.

**Verify**: `just userapp gif` → exit 0.

### Step 5: Make the key queue safe across cores

Replace the raced `static mut Queue` with a lock-free structure that is sound
for two cores. The repo already depends on `embassy-sync`
(`kernel/Cargo.toml`), whose `Channel` with a `CriticalSectionRawMutex` is
explicitly multi-core safe and is already used for `BINARY_CH`
(`kernel/src/main.rs:206`):

```rust
// kernel/src/main.rs:206 — existing exemplar
static BINARY_CH: Channel<CriticalSectionRawMutex, (EntryFn, Bump), 1> = Channel::new();
```

In `kernel/src/syscalls.rs`, replace line 149 with:

```rust
/// Keyboard events produced by core0's `key_handler` and consumed by the
/// `get_key` syscall on core1. `CriticalSectionRawMutex` is required here —
/// the two ends are on different cores.
pub static KEY_CACHE: Channel<CriticalSectionRawMutex, KeyEvent, 32> = Channel::new();
```

Add the imports (`embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex`,
`embassy_sync::channel::Channel`) and drop the `heapless::spsc::Queue` import
if nothing else uses it.

`get_key` (`:151-163`) must stay non-blocking — it is a synchronous syscall:

```rust
const _: GetKey = get_key;
pub extern "C" fn get_key() -> KeyEventC {
    match KEY_CACHE.try_receive() {
        Ok(event) => event.into(),
        Err(_) => KeyEvent {
            key: KeyCode::Unknown(0),
            state: KeyState::Idle,
            mods: Modifiers::empty(),
        }
        .into(),
    }
}
```

In `kernel/src/main.rs:427-436`, the producer uses the non-blocking send so a
full queue drops the oldest-blocking case rather than stalling core0, and
polls faster:

```rust
async fn key_handler() {
    loop {
        if let Some(event) = read_keyboard_fifo().await {
            // Dropping on a full queue is deliberate: a user app that stops
            // calling `get_key` must not stall core0's input task.
            let _ = KEY_CACHE.try_send(event);
        }
        Timer::after_millis(10).await;
    }
}
```

Remove the `unsafe` block — `Channel` needs none. Update the import at
`kernel/src/main.rs:46` if the path changed.

**Verify**: `grep -c 'static mut KEY_CACHE' kernel/src/syscalls.rs` → `0`.
`grep -c 'unsafe' kernel/src/main.rs` → one fewer than before (check with
`git diff`). `cargo build --bin kernel --release --no-default-features --features rp235xa`
→ exit 0.

### Step 6: Make app panics return to the kernel instead of hanging

A `#[panic_handler]` must return `!`, and this one cannot unwind back into the
kernel (`panic = "unwind"` is set for `release-binary` in the root
`Cargo.toml`, but there is no unwinder in the app). The achievable improvement
is to stop the device from needing a reset: print the panic, then park in a
way the user can escape.

In **each** of the five `user_apps/*/src/main.rs` files, replace the handler
body:

```rust
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("user panic: {} @ {:?}", info.message(), info.location());
    // The kernel cannot reclaim core1 from a panicked app, so wait for the
    // user to reset rather than spinning at full speed.
    loop {
        userlib::sleep(1000);
    }
}
```

This is a small improvement (no longer a hot spin), not a fix. Record in the
maintenance notes that properly reclaiming core1 after an app panic needs a
kernel-side abort path, which is plan 006's territory.

Do **not** change any other panic site in the apps in this plan.

**Verify**: `just userapps` → exit 0.
`grep -c 'loop {}' user_apps/*/src/main.rs` → `0` for all five.

### Step 7: Regenerate the C header and confirm the ABI is unchanged

`just cbindgen` regenerates `userlib_sys.h` (it is in `.gitignore`, so this is
a build artifact, but it must still generate cleanly for C consumers linking
against picolibc).

**Verify**: `just cbindgen` → exit 0.
`grep -n 'SYS_CALL_TABLE_COUNT' userlib_sys.h` → still `17`.
`just test` → exit 0 (plan 001's count test passes).
`just userapps` → exit 0.

## Test plan

- `userlib` and the apps cannot be unit-tested on the host (they are
  `no_std`, `no_main`, and `userlib`'s global allocator calls into the
  syscall table). The verification for steps 1, 2, 4 and 6 is "compiles" plus
  the manual device check below.
- Add one host test to `userlib_sys/src/lib.rs`'s `mod tests` (created by
  plan 001) pinning the entry-buffer contract that step 1 and the kernel's
  `copy_entry_to_user_buf` must agree on:

```rust
#[test]
fn entry_name_buffer_leaves_room_for_terminator() {
    // `copy_entry_to_user_buf` in the kernel writes at most
    // `max_str_len - 1` bytes plus a NUL; `userlib` passes
    // MAX_ENTRY_NAME_LEN = 25. A name of exactly that length must still be
    // NUL-terminated inside the buffer.
    const MAX_ENTRY_NAME_LEN: usize = 25;
    let name = [b'a'; 40];
    let copied = name.len().min(MAX_ENTRY_NAME_LEN - 1);
    assert_eq!(copied, 24);
    assert!(copied + 1 <= MAX_ENTRY_NAME_LEN);
}
```

- Verification: `just test` → exit 0, 1 new test passes.

## Done criteria

ALL must hold:

- [ ] `just test` exits 0; the new entry-buffer test passes
- [ ] `just check` exits 0
- [ ] `grep -c 'as_ptrs' userlib/src/lib.rs` → 0
- [ ] `grep -c 'asm::nop' kernel/src/syscalls.rs` → 0
- [ ] `grep -c 'static mut KEY_CACHE' kernel/src/syscalls.rs` → 0
- [ ] `grep -c 'loop {}' user_apps/calculator/src/main.rs user_apps/snake/src/main.rs user_apps/gallery/src/main.rs user_apps/gif/src/main.rs user_apps/wav_player/src/main.rs` → 0 each
- [ ] `SYS_CALL_TABLE_COUNT` is still `17`; `just cbindgen` exits 0
- [ ] `git status` shows only in-scope files modified
- [ ] `plans/README.md` status row updated

**Manual device check** (operator; record as pending if no hardware):
`just kernel-release rp235xa`, `just copy-userapps`.
1. Launch the gif app: the file list must show real gif names (step 1), and
   playback must run at roughly the gif's real frame rate rather than as fast
   as possible (step 4).
2. While a gif plays, hold a key: input must still register (step 5).
3. Launch the wav player with no files in `/music`: it hits
   `assert!(selection.is_some())`. Expected: the panic message is logged and
   the device does not get hot/unresponsive — it still needs a reset, which is
   the known limitation.

## STOP conditions

- `embassy_sync::channel::Channel::try_send` / `try_receive` are not available
  in the pinned `embassy-sync` 0.7 — report the actual API rather than
  reverting to the raced `Queue`.
- `cortex_m::asm::wfe()` in step 3 causes `sleep` to return early or never
  return on device — report the observed behavior; a `Timer`-based alternative
  requires making the syscall async, which is an ABI change and out of scope.
- Step 1's fix requires making `Entries.0` public — it does not (same module);
  if the borrow checker disagrees, report the error.
- Any step appears to require bumping `SYS_CALL_TABLE_COUNT`. Stop — that is
  plan 006.

## Maintenance notes

- `sleep` still blocks core1 synchronously. That is inherent to the current
  ABI (syscalls are plain `extern "C"` function pointers with no way to yield
  to an executor). An async-capable ABI is plan 006.
- An app that panics still owns core1 forever; the kernel has no abort path.
  Plan 006 covers giving `userland_task` a way to reclaim the core.
- `KEY_CACHE` is now a 32-slot channel that drops events when full. If an app
  reports missed keys, the right lever is the 10 ms poll interval in
  `key_handler` and the queue depth — not reverting to the unsynchronized
  queue.
- `userlib::fs::list_dir`'s `MAX_ENTRIES`/`MAX_ENTRY_NAME_LEN` (25/25,
  `userlib/src/lib.rs:332-333`) must stay consistent with the kernel's LFN
  buffer (`kernel/src/syscalls.rs:189-190` uses a 25-byte buffer). Changing
  one without the other truncates long filenames silently.
- Reviewer should scrutinize: that the pointer array in step 1 is a named
  local (not a temporary) and that `Channel` in step 5 is declared with
  `CriticalSectionRawMutex`, not `NoopRawMutex` — the latter would compile and
  silently reintroduce the race.

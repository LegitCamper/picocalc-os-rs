# Plan 006: Design ABI v2 — versioned, error-returning syscalls and a core1 abort path

> **Executor instructions**: This is a **design/spike plan**. Its deliverable
> is a written design document plus one throwaway prototype branch — NOT a
> finished migration. Do not change the shipped ABI in this plan. Follow the
> steps, answer every question in "Questions the design must answer", and stop
> at the decision point for human review. When done, update the status row for
> this plan in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat b619dfa..HEAD -- userlib_sys/src/lib.rs kernel/src/syscalls.rs kernel/src/main.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: L
- **Risk**: HIGH (the output is a design; the eventual migration is the risky part)
- **Depends on**: plans/002-syscall-arg-validation.md, plans/004-framebuffer-bounds-and-races.md, plans/005-userland-interface-fixes.md
- **Category**: direction
- **Planned at**: commit `b619dfa`

## Why this matters

Plans 002–005 remove the crashes that the current ABI makes *easy*. They
cannot remove the ones it makes *structural*:

- **No syscall can report an error.** `write_file` returns `()`. `read_file`
  returns `0` for "empty file", "not found", "card busy" and "bad path" alike.
  `fill_rect` returns nothing. So every user app either ignores failure or
  panics, and the kernel's only honest options are "silently do nothing" or
  "panic". Plan 002 has to invent "nothing happened" values precisely because
  the ABI has no error channel.
- **No app can be stopped.** `userland_task` calls `entry()` and blocks until
  it returns (`kernel/src/main.rs:209-253`). A panicked or looping app owns
  core1 forever; plan 005 can only make the hang cheaper, not survivable.
- **No version negotiation.** A binary built against a different syscall table
  is detected only by a size comparison on one symbol
  (`kernel/src/elf.rs:220-223`) and rejected with
  `SyscallTableSizeMismatch` — which cannot distinguish "older build" from
  "corrupt file", and silently accepts a table of the *same size but different
  order*.
- **Every syscall is synchronous and blocking.** `sleep` blocks core1,
  `send_audio_buffer` spin-waits on core0's audio task, and drawing syscalls
  mutate a framebuffer core0 is concurrently reading.

The README says the focus is "finding bugs in ffi, and making sure the kernel
is as stable as possible". These four are the ffi design, not bugs in it.
Getting the shape right before more apps are ported is the leverage; getting
it wrong means breaking every `.bin` on every user's SD card twice.

## Current state

**The ABI, in full.** `userlib_sys/src/lib.rs:14-42`:

```rust
pub type EntryFn = fn();

pub const SYS_CALL_TABLE_COUNT: usize = 17;
const _: () = assert!(SYS_CALL_TABLE_COUNT == SyscallTable::COUNT);

#[derive(Clone, Copy, EnumIter, EnumCount)]
#[repr(u8)]
pub enum SyscallTable {
    Alloc = 0,
    Dealloc = 1,
    PrintString = 2,
    SleepMs = 3,
    GetMs = 4,
    DrawIter = 5,
    GetKey = 6,
    GenRand = 7,
    ListDir = 8,
    ReadFile = 9,
    WriteFile = 10,
    FileLen = 11,
    ReconfigureAudioSampleRate = 12,
    AudioBufferReady = 13,
    SendAudioBuffer = 14,
    FillRect = 15,
    Blit = 16,
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".syscall_table")]
pub static mut SYS_CALL_TABLE: [usize; SYS_CALL_TABLE_COUNT] = [0; SYS_CALL_TABLE_COUNT];
```

Each app-side stub transmutes a raw `usize` from that table and calls it, e.g.
(`userlib_sys/src/lib.rs:68-73`):

```rust
#[unsafe(no_mangle)]
pub extern "C" fn alloc(layout: CLayout) -> *mut u8 {
    let f: Alloc = unsafe { core::mem::transmute(SYS_CALL_TABLE[SyscallTable::Alloc as usize]) };
    f(layout)
}
```

A zeroed slot (any syscall the kernel failed to patch) transmutes to a null
function pointer and is called → immediate hard fault. There is no "is this
slot populated" check anywhere.

**How the table is populated.** `kernel/src/elf.rs:174-260` scans the symbol
table for `SYS_CALL_TABLE`, checks only its *size*, and writes the kernel's
function pointers in `SyscallTable::iter()` order:

```rust
// kernel/src/elf.rs:220-223
let expected_size = SYS_CALL_TABLE_COUNT * size_of::<usize>();
if sym.st_size as usize != expected_size {
    return Err(LoadError::SyscallTableSizeMismatch);
}
```

Verified against a real binary: `.syscall_table` is a `NOBITS` section at
`0xbd80`, size `0x44` = 68 = 17 × 4, and `SYS_CALL_TABLE` is a 68-byte
`OBJECT` symbol there. So index order is the entire contract — inserting a
syscall anywhere but the end silently repoints every later call.

**App lifecycle.** `kernel/src/main.rs:205-253`:

```rust
static BINARY_CH: Channel<CriticalSectionRawMutex, (EntryFn, Bump), 1> = Channel::new();

#[embassy_executor::task]
async fn userland_task() {
    let recv = BINARY_CH.receiver();
    loop {
        let (entry, _bump) = recv.receive().await;
        ENABLE_UI.store(false, Ordering::Release);
        UI_CHANGE.signal(());
        clear_selection().await;
        MSC_SHUTDOWN.signal(());
        unsafe { MS_SINCE_LAUNCH = Some(Instant::now()) };
        entry();
        // ... re-enable UI, clear audio, clear framebuffer ...
    }
}
```

`entry()` is a plain synchronous call on core1's executor. There is no
timeout, no watchdog scoped to the app (the global one at
`kernel/src/main.rs:106-123` is fed by core0 every 2 s regardless), and no way
to interrupt. Core1's stack is a fixed `Stack<16384>`
(`kernel/src/main.rs:91`) shared with the executor.

**Resources the app implicitly owns while running.** `FRAMEBUFFER`
(`kernel/src/display.rs:34`, `static mut`, also read by core0),
`AUDIO_BUFFER` (`kernel/src/audio.rs:23`, `static mut`, swapped by core0's
`audio_handler`), `KEY_CACHE` (`kernel/src/syscalls.rs:149`), the `SDCARD`
mutex (`kernel/src/storage.rs:27`, contended with core0's 5-second
`prog_search_handler` poll and the USB MSC path), and the kernel's global
`talc` allocator arena (`kernel/src/main.rs:96-103`) — app allocations come
out of kernel memory with no quota.

### Conventions to match

- ABI types live in `userlib_sys`, are `#[repr(C)]`, and are paired:
  `pub type X = extern "C" fn(..)` on the app side,
  `const _: X = func;` on the kernel side.
- Compile-time invariants use `const _: () = assert!(...)`
  (`userlib_sys/src/lib.rs:16`, `kernel/src/audio.rs:18`).
- `userlib` is the safe wrapper; `userlib_sys` is the raw boundary; apps use
  only `userlib`.
- Host-testable logic goes in `userlib_sys` (plan 001).
- `cbindgen` generates `userlib_sys.h` for C consumers (`just cbindgen`,
  config in `cbindgen.toml`) — any ABI design must stay cbindgen-expressible.
  Note the current output is C++ (`enum class`, `#include <new>`) despite
  `language = "C"`; a `RngRequest` tagged union appears there too. The design
  must say whether C interop is a real requirement or an aspiration.

## Commands you will need

| Purpose       | Command                                                                 | Expected on success |
|---------------|-------------------------------------------------------------------------|---------------------|
| Host tests    | `just test`                                                             | exit 0, tests pass  |
| Kernel builds | `cargo build --bin kernel --release --no-default-features --features rp235xa` | exit 0        |
| Apps build    | `just userapps`                                                         | exit 0              |
| Regen header  | `just cbindgen`                                                         | exit 0              |
| Inspect ABI   | `arm-none-eabi-readelf -sW target/thumbv8m.main-none-eabihf/release-binary/calculator \| grep SYS_CALL_TABLE` | one 68-byte OBJECT symbol |
| Full gate     | `just check`                                                            | exit 0              |

## Scope

**In scope** (this plan produces exactly these):
- `plans/006-abi-v2-design.md` — the design document (new file, written by you)
- A prototype on a throwaway branch `spike/abi-v2`, not merged

**Out of scope** (do NOT do in this plan):
- Changing `SYS_CALL_TABLE_COUNT`, the `SyscallTable` enum, or any syscall
  signature on `main`/the working branch.
- Touching `kernel/src/syscalls.rs`, `kernel/src/elf.rs`,
  `kernel/src/framebuffer.rs` or `userlib/src/lib.rs` outside the spike
  branch — plans 002–005 own those files and their drift checks must keep
  passing.
- Migrating the five user apps. The design must *describe* the migration; the
  migration itself is a later plan.
- Adding an MPU/privilege-separation design. The RP2350 has an MPU, but
  scoping that is a separate research question — note it as future work only.

## Git workflow

- Design doc on branch `advisor/006-abi-v2-design`, one commit.
- Prototype on a separate branch `spike/abi-v2` branched from the same base.
  Mark it clearly as throwaway in the commit message. Do NOT merge it.
- Do NOT push unless the operator asks.

## Steps

### Step 1: Inventory the current boundary exhaustively

Produce a table in the design doc with one row per syscall (all 17), each
listing: index, app-side signature, kernel-side implementation
(`file:line`), what it returns on failure today, what it *should* be able to
report, whether it blocks, and which shared resource it touches. Source the
signatures from `userlib_sys/src/lib.rs` and the implementations from
`kernel/src/syscalls.rs`.

This table is the design's foundation — do not summarize it.

**Verify**: the table has 17 rows and every `file:line` reference resolves
(spot-check 5 with `sed -n 'N,Mp'`).

### Step 2: Decide the error-reporting shape

Pick one and justify it against the alternatives in writing:

- **(a) Negative-`isize` returns** — C-ergonomic, cheap, no ABI type churn,
  but conflates valid large lengths with errors and needs every signature
  widened.
- **(b) `#[repr(C)] struct SysResult { code: i32, value: usize }`** — explicit,
  cbindgen-expressible, costs a register pair per call.
- **(c) Out-parameter for the error** (`*mut SysError`) — least disruptive to
  existing signatures, easiest to get wrong from C, another pointer to
  validate.

Requirements the choice must meet: distinguishes at minimum `Ok`,
`NotFound`, `Busy`, `InvalidArgument`, `PermissionDenied`, `OutOfMemory`,
`Unsupported`; expressible by cbindgen; mappable to `Result<T, E>` in
`userlib` so apps use `?` instead of `unwrap()`; and ABI-stable (adding an
error code later must not break existing binaries).

Write the chosen `SysError` enum as real Rust, with explicit discriminants.

**Verify**: the design doc shows, for three concrete syscalls (`read_file`,
`write_file`, `fill_rect`), the before/after signature and the `userlib`
wrapper that turns it into a `Result`.

### Step 3: Design version negotiation

Replace size-matching with an explicit handshake. The minimum viable design:
a `#[repr(C)]` header struct at a known offset in the `.syscall_table`
section containing a magic value, an ABI major/minor version, and the entry
count — so the loader can reject an incompatible binary with a specific error
and, for a minor-version-older binary, populate only the slots it knows about.

Specify: the struct layout, the magic constant, the version-compatibility rule
(what major/minor mean, what the loader does for each combination), what
happens to unknown slots (the design must ensure an unpopulated slot cannot be
called — today a zeroed slot is transmuted and called, which hard-faults), and
how `kernel/src/elf.rs:174-260` changes to read it.

Also specify how `SyscallTable` gains entries safely: append-only, with a
`const _: () = assert!(...)` guard pinning each existing discriminant so a
reorder fails the build.

**Verify**: the design doc states explicitly what the loader does for each of:
same version, app older minor, app newer minor, app different major, bad
magic, missing symbol.

### Step 4: Design the app lifecycle and abort path

Specify how the kernel regains core1. Investigate and document the real
constraints before choosing:

- Does `embassy_executor` on core1 permit the app to be run as a *task* that
  can be dropped, or is the current blocking `entry()` call the only option
  given `EntryFn = fn()`?
- Can the RP2350's second core be reset and re-launched with `spawn_core1`
  while core0 keeps running? Read `embassy_rp::multicore` and record what is
  actually supported. Note `CORE1_STACK` is a `static mut Stack<16384>`
  (`kernel/src/main.rs:91`) that would have to be re-initialized.
- What does the app-facing contract become — may `entry()` return a status?
  Is there a cooperative `yield`/`should_exit` syscall apps are expected to
  poll (cheap, requires app cooperation), a watchdog scoped to the app
  (needs a per-app timeout policy), or a hard core reset (works on a
  panicked app, but leaks whatever the app held)?

Then specify resource reclamation for each path: the `Bump` arena
(`kernel/src/main.rs:211` holds it as `_bump` for exactly this reason), any
`alloc` the app made from the kernel's global `talc` arena (currently
unaccounted — the design must say whether apps get a quota or a separate
arena), the `SDCARD` mutex if the app died holding it, `FRAMEBUFFER`,
`AUDIO_BUFFER`, and the audio flags that `userland_task` currently resets by
hand (`kernel/src/main.rs:230-241`).

**Verify**: the design doc contains a state machine (Idle → Loading → Running
→ Exited/Aborted → Idle) naming, for every transition, which resources are
reclaimed and by which core.

### Step 5: Prototype the riskiest part only

On branch `spike/abi-v2`, implement the **version handshake and one
error-returning syscall** (`read_file`) end to end: `userlib_sys` types, the
kernel implementation, the loader change, and the `userlib` `Result` wrapper.
Convert exactly one app (`user_apps/gallery`, the smallest consumer of
`read_file`) to the new wrapper.

The point is to measure, not to ship. Record in the design doc:
- flash and RAM delta on the kernel (`cargo build` + `arm-none-eabi-size`),
- flash delta on the converted app,
- whether cbindgen still emits a usable header (`just cbindgen`),
- how many lines the app conversion touched, multiplied by five apps.

**Verify**: on the spike branch, `just userapp gallery` and the kernel build
both exit 0, and the design doc records the four measurements above.

### Step 6: Write the migration plan and stop

The design doc ends with a staged migration proposal: which plan numbers, in
what order, what each one breaks, and how users with existing `.bin` files are
handled (the loader must reject an old binary with a clear on-screen message
rather than hard-faulting — note plan 003 adds the error path this needs).

Then **stop and report**. Do not begin the migration.

**Verify**: `plans/006-abi-v2-design.md` exists, covers steps 1–6, and
answers every question in the next section. `just check` on the
`advisor/006-abi-v2-design` branch → exit 0 (the design doc is the only
change, so nothing can break).

## Questions the design must answer

Answer each explicitly; "unknown" is acceptable only with a note on how to
find out.

1. Is C interop (via `cbindgen`/picolibc) a real requirement, or is
   `userlib_sys.h` vestigial? The answer decides whether `SysResult` may be a
   Rust enum. Evidence either way: `just newlib` exists and
   `README.md` lists `picolib/` as providing libc symbols, but no C app exists
   in-tree.
2. Do apps get their own heap, or keep allocating from the kernel's `talc`
   arena (`kernel/src/main.rs:96-103`)? If shared, what stops an app from
   OOM-ing the kernel?
3. Can core1 be reset and relaunched at runtime on RP2350 with the pinned
   `embassy-rp` 0.8? If not, what is the fallback for a panicked app?
4. Does the framebuffer stay a `static mut` shared across cores, or does the
   app get a buffer it owns and submits? (Plan 004 only papers over the
   race with a writer counter — the design must say what replaces it.)
5. What is the version-compatibility promise? Specifically: will a binary
   built today still load after ABI v2 ships, or is this a clean break with a
   clear error?
6. Does `sleep` stay blocking? If the app is to be yieldable, the syscall
   table needs an async-capable mechanism, which changes every signature.

## Done criteria

ALL must hold:

- [ ] `plans/006-abi-v2-design.md` exists and contains: the 17-row syscall
      inventory, the chosen error shape with alternatives rejected in writing,
      the version-handshake struct and loader rules, the lifecycle state
      machine, the spike measurements, and the staged migration proposal
- [ ] Every one of the six "Questions the design must answer" has an explicit
      answer
- [ ] Branch `spike/abi-v2` exists, builds (`just userapp gallery` + kernel
      build exit 0), and is **not** merged
- [ ] `git status` on `advisor/006-abi-v2-design` shows only
      `plans/006-abi-v2-design.md` and this file's status row added/modified
- [ ] `SYS_CALL_TABLE_COUNT` is still `17` on the non-spike branch
- [ ] `just check` exits 0 on the non-spike branch
- [ ] `plans/README.md` status row updated

## STOP conditions

- The spike shows the flash cost of the chosen error shape is prohibitive
  (kernel or app growth that does not fit). Report the numbers and propose a
  cheaper shape rather than proceeding.
- `embassy-rp` 0.8 turns out to have no supported way to reset core1. Report
  it; the lifecycle design then has to be cooperative-only, which changes
  step 4's conclusion.
- You find yourself editing files outside the spike branch and
  `plans/006-abi-v2-design.md`. Stop.
- Plans 002–005 have not landed yet and the spike's base no longer matches the
  "Current state" excerpts. Stop and report — this design depends on knowing
  which crashes were already fixed.

## Maintenance notes

- This plan deliberately produces no shipped behavior change. Its value is
  that the eventual ABI break happens **once**, with the error channel,
  version handshake and abort path designed together — they interact (an
  abort path needs an error code; a version handshake needs somewhere to put
  the magic; both change the loader).
- Whoever executes the migration must update, in lockstep: the
  `SyscallTable` enum, `SYS_CALL_TABLE_COUNT`, `kernel/src/syscalls.rs`,
  `kernel/src/elf.rs`'s patcher, `userlib`, all five apps, `selection_ui`,
  and `userlib_sys.h` via `just cbindgen`.
- Explicitly deferred: MPU-based memory protection (the RP2350 has one; using
  it would make the loader's bounds checks defense-in-depth rather than the
  only line), and multi-app/multitasking — nothing in this design should
  foreclose either, and the design doc should say so.

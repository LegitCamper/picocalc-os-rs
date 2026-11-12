# ABI v2 design and isolated prototype

Status: design/spike complete; production migration requires review. The shipped
tree still uses the 18-slot table. The prototype is retained as
`plans/006-abi-v2-prototype.patch`, not applied to production sources. No branch
or commit was created; builds ran in an isolated source copy under `/tmp`.

## Current boundary inventory

Signatures below describe table entries, not convenience wrappers. `usize` and
pointers are 32 bits on the target. Input pointers are raw; the kernel validates
complete ranges against the loaded image, usable app stack, or tracked live
allocations. Filesystem output buffers must not overlap their inputs. The RNG
request tag is checked before decoding its enum. These checks are not an MPU
sandbox: apps still execute privileged native code, share a stack with syscall
frames, and can bypass the table or forge pointers into app-visible storage.

| Slot | Entry signature | Kernel implementation | Current failure / required status | Blocking and shared state |
|---|---|---|---|---|
| 0 | `Alloc(CLayout) -> *mut u8` | `kernel/src/syscalls.rs:72` | Null + InvalidArgument/NoMemory; retain explicit cause | Allocator lock, allocation registry |
| 1 | `Dealloc(*mut u8, CLayout)` | `kernel/src/syscalls.rs:81` | No-op + InvalidArgument on foreign/mismatched/double free | Allocator lock, allocation registry |
| 2 | `PrintString(*const u8, usize)` | `kernel/src/syscalls.rs:90` | No-op + InvalidArgument; absent logging is not an error | Optional logging backend |
| 3 | `SleepMs(u64)` | `kernel/src/syscalls.rs:101` | Invalid duration rejected; needs Interrupted in v2 | Blocks app using timer/WFE, not core0 |
| 4 | `GetMs() -> u64` | `kernel/src/syscalls.rs:112` | Zero before launch; otherwise infallible | Core1 launch timestamp |
| 5 | `DrawIter(*const CPixel, usize)` | `kernel/src/syscalls.rs:117` | No-op on invalid range; needs direct InvalidArgument/Unavailable | Framebuffer mutex, potentially waits for DMA |
| 6 | `GetKey(*mut KeyEventC)` | `kernel/src/syscalls.rs:170` | InvalidArgument; valid empty queue writes Idle | Brief channel critical section |
| 7 | `GenRand(*mut RngRequest)` | `kernel/src/syscalls.rs:188` | InvalidArgument for bad tag/range/overlap | Hardware RNG, work proportional to length |
| 8 | `ListDir(path, len, entries, count, capacity) -> usize` | `kernel/src/syscalls.rs:289` | Zero + InvalidArgument/NotFound/Busy/Io/NoMemory/BufferTooSmall | SD try-lock, synchronous traversal |
| 9 | `ReadFile(path, len, offset, out, capacity) -> usize` | `kernel/src/syscalls.rs:393` | Zero + status; zero also means EOF | SD try-lock and synchronous read |
| 10 | `WriteFile(path, len, offset, data, len)` | `kernel/src/syscalls.rs:430` | No return value + status; v2 needs actual progress | SD try-lock, write, flush and close |
| 11 | `FileLen(path, len) -> usize` | `kernel/src/syscalls.rs:451` | Zero + status; zero also means empty file | SD try-lock and synchronous metadata read |
| 12 | `ReconfigureAudioSampleRate(u32)` | `kernel/src/syscalls.rs:458` | InvalidArgument/Busy through status | Brief mailbox critical section |
| 13 | `AudioBufferReady() -> bool` | `kernel/src/syscalls.rs:462` | False means pending/resetting, not fatal failure | Brief mailbox critical section |
| 14 | `SendAudioBuffer(*const u8, usize)` | `kernel/src/syscalls.rs:466` | InvalidArgument/Busy through status | Bounded copy of exactly 2048 bytes; no wait for playback |
| 15 | `FillRect(u16,u16,u16,u16,u16)` | `kernel/src/syscalls.rs:132` | Clipped/empty rectangles succeed; needs Unavailable status | Framebuffer mutex, potentially waits for DMA |
| 16 | `Blit(u16,u16,u16,u16,*const u16,usize)` | `kernel/src/syscalls.rs:142` | InvalidArgument for bad extent/range; partial input allowed | Framebuffer mutex, potentially waits for DMA |
| 17 | `GetLastError() -> i32` | `kernel/src/syscalls.rs:46` | No failure; shared status is unsuitable for reentrant calls | Atomic load on single app thread |

`GetKey` now takes an explicit output pointer: returning the 12-byte struct
directly from the kernel hid an unchecked destination in the ARM C calling
convention. The app-side convenience function still returns a value, using a
local initialized output buffer. Old 17-slot apps must be rebuilt.

## Error representation

Choose negative scalar returns, widened from `isize` to `i64`:

```rust
pub type SysResult = i64;

#[repr(i32)]
pub enum SysError {
    Ok = 0,
    InvalidArgument = 1,
    NotFound = 2,
    Busy = 3,
    Io = 4,
    NoMemory = 5,
    Unsupported = 6,
    BufferTooSmall = 7,
    PermissionDenied = 8,
    Interrupted = 9,
    Unavailable = 10,
}
```

Nonnegative values are successful payloads; negative values are error codes.
This represents every 32-bit file length/address without the 2 GiB ambiguity of
negative `isize`. `GetMs` can retain its infallible `u64` return. Large composite
outputs use explicitly validated output pointers. Unknown negative codes map to
`Io` in old wrappers, never a transmuted Rust enum.

A C struct result is explicit but may introduce an implicit result destination
under the target C ABI, repeating the keyboard bug. An error out-parameter adds
another writable pointer and a second place for callers to forget to check.
An integer return avoids both. `GetLastError` remains only in the legacy ABI;
v2 callers never depend on shared mutable error state.

Three concrete migrations:

| Call | Current return | V2 return meaning | Safe wrapper |
|---|---|---|---|
| `read_file` | `usize` plus `GetLastError` | `SysResult`: bytes read, zero at EOF, negative error | `Result<usize, SysError>` |
| `write_file` | `()` plus `GetLastError` | `SysResult`: bytes accepted, negative error | `Result<usize, SysError>`; loop/flush policy explicit |
| `fill_rect` | `()` | `SysResult`: zero on success, negative error | `Result<(), SysError>` |

The read wrapper in the prototype is executable code. Equivalent future wrappers
for the other two calls use the same checked decoder:

```rust
pub fn read_file(path: &str, offset: usize, output: &mut [u8]) -> Result<usize, SysError> {
    // SAFETY: borrowed path and exclusive output remain live for this synchronous call.
    let result = unsafe { raw::read_file(path.as_ptr(), path.len(), offset, output.as_mut_ptr(), output.len()) };
    decode_result(result)
}

pub fn write_file(path: &str, offset: usize, input: &[u8]) -> Result<usize, SysError> {
    // SAFETY: both immutable slices remain live for this synchronous call.
    let result = unsafe { raw::write_file(path.as_ptr(), path.len(), offset, input.as_ptr(), input.len()) };
    decode_result(result)
}

pub fn fill_rect(x: u16, y: u16, width: u16, height: u16, color: u16) -> Result<(), SysError> {
    decode_result(raw::fill_rect(x, y, width, height, color)).map(|_| ())
}
```

`decode_result` in the retained patch checks negation and integer conversion,
including `i64::MIN`. Write errors must not imply rollback: a media failure can
follow a partial write. The full migration needs a documented partial-write and
flush contract before exposing the new write wrapper.

## Version negotiation

Use a separate, immutable `SYS_CALL_ABI` object in `.syscall_abi`; leave
`SYS_CALL_TABLE` as a writable array of pointers. This avoids changing array
indexing to accommodate a prefix header.

```rust
#[repr(C)]
pub struct AbiHeader {
    pub magic: u32,       // little-endian bytes "PCOS"
    pub major: u16,       // 2
    pub minor: u16,       // 0 initially
    pub entries: u16,
    pub reserved: u16,    // must be zero
}
const _: () = assert!(core::mem::size_of::<AbiHeader>() == 12);
```

The loader validates the symbol type, exact 12-byte size, alignment, containing
allocated section and loaded range before interpreting these bytes. It rejects
conflicting duplicate descriptors. The declared entry count must match the
actual table size and the known layout for that minor version. Counts are never
trusted allocation sizes. Initialization must finish before app entry.

| App vs kernel | Policy |
|---|---|
| Same major/minor | Validate the exact known slot layout and fill every slot |
| Older minor, same major | Accept only a documented append-only prefix; fill its entire table |
| Newer minor | Reject as incompatible; never leave unknown/null slots callable |
| Different major | Reject with an ABI-mismatch message |
| Bad magic/reserved fields/count | Reject malformed descriptor |
| Missing descriptor or table | Reject legacy/corrupt app; offer rebuild guidance |

Adding slots is append-only within a major. Pin all existing discriminants with
compile-time assertions (already done for the current table). Type changes
require a major version even when slot counts happen to match. A production
linker script must `KEEP` the descriptor; the prototype retains it through an
explicit reference in the read stub. No header means no legacy fallback based
only on size. The prototype recognizes only the declared v2.0 layout, not
invented future minor layouts.

## Lifecycle and ownership

Evidence: pinned `embassy-rp 0.8.0/src/multicore.rs`, especially `spawn_core1`
and `pause_core1`/`resume_core1`; the checked-out `kernel/src/main.rs` userland
task; `kernel/src/utils.rs` synchronous future waiter.

- `entry()` runs synchronously inside one executor poll. Dropping or cancelling
  its async wrapper cannot preempt an app that never returns.
- The HAL startup routine forces PROC1 off/on, then performs its boot handshake.
  It consumes the unique core peripheral and a static stack. It does not offer
  a resource-aware runtime app-reset operation. Calling it again through stolen
  ownership does not make the old executor, `StaticCell`, stack or held locks
  safe to reuse.
- Pause/resume wait for FIFO acknowledgements. They neither unwind app frames
  nor reclaim resources; disabled interrupts or a wedged core can prevent an
  acknowledgement. Resetting during a critical section can strand a shared
  spinlock or allocator lock and stall core0 too.
- Current app panics print without allocating, then sleep in one-second waits.
  Core0 keeps running, but the app still owns core1. The core0-fed watchdog is
  not an app watchdog. Current recovery remains a device reset.

Initial v2 lifecycle should be cooperative: a core0 stop request sets a flag,
`ShouldExit` exposes it, and apps return an `extern "C" fn() -> i32` status from
entry. Timed sleep checks cancellation and returns `Interrupted`. This is not
preemption. A non-cooperative app or panic requires a controlled whole-device
reset until isolated syscall execution and a hardware-tested reset/relaunch
protocol exist. Do not add a nonlocal jump out of arbitrary Rust/FFI frames.

| Transition | Ownership and cleanup |
|---|---|
| Idle to Loading | Core0 holds SD access and builds a fallible image arena; failure drops it and leaves menu usable |
| Loading to Running | Transfer image ownership to core1; core0 acknowledges UI suspension; core1 registers image/stack and clears stale keys |
| Running to StopRequested | Core0 sets cancellation flag; no memory is freed while app can still execute |
| Running/StopRequested to Exited | App returns; core1 frees tracked leftover app allocations, awaits audio reset acknowledgement, clears framebuffer, drops image, then reenables UI |
| Running to Aborted | Not a safe shipped transition for arbitrary native execution; forced core reset requires lock/DMA quiescence and a separate executor lifecycle protocol |
| Exited to Idle | Core0 resumes menu only after ownership and audio cleanup complete |

The current registry tracks at most 256 live app allocations and rejects wrong
layouts, foreign pointers and duplicate frees. Normal exit reclaims leaks.
There is no byte quota yet, and panic does not run cleanup. V2 should add a
separate budgeted app arena or byte quota with reserved kernel headroom; metadata
must stay in internal SRAM. Do not reset a heap while DMA or syscall frames can
still refer to it.

Framebuffer access already uses one cross-core mutex held through each display
transfer; it no longer uses a writer counter or `FB_PAUSED`. Audio already copies
packets into a bounded mailbox and lets core0 own DMA storage. Ordinary filesystem
errors close/drop file and directory handles before releasing the SD lock. These
facts make normal return safe, not an asynchronous reset at an arbitrary PC.

## Prototype results

The retained patch adds descriptor validation, scalar-result decoding, and the
read syscall end to end. Other table entries remain legacy: this is a mixed
experimental build, not a production v2 release. Never distribute its binaries
as the final v2 ABI. Gallery already uses the safe `Result` wrapper, so its
source needs zero conversion lines; changing that wrapper is sufficient.

Observed with the repository-pinned toolchain, default `rp235xa` release build,
and gallery `release-binary` build. `arm-none-eabi-size` reports bytes:

| Artifact/section | Current implementation | Prototype | Delta |
|---|---:|---:|---:|
| Kernel text | 139548 | 140064 | +516 |
| Kernel data | 0 | 0 | 0 |
| Kernel BSS | 494300 | 494300 | 0 |
| Gallery text | 23879 | 23791 | -88 |
| Gallery data | 912 | 916 | +4 |
| Gallery BSS | 12362 | 12362 | 0 |

Gallery loadable text+data changes by -84 bytes. These are static build sizes,
not measured stack peaks, heap usage, launch latency or hardware observations.
The descriptor appears as a 12-byte `OBJECT` symbol in rebuilt gallery.

Checks completed in the isolated copy:

- `cargo build --bin kernel --release --no-default-features --features rp235xa`
- `just userapp gallery`
- `just check-header` (generated header accepted by C11 and C++17 compilers)
- `just test` (including handshake and scalar-result regression tests)
- `git apply --check plans/006-abi-v2-prototype.patch` against the production tree

To repeat, copy the current source tree without `.git` or `target` to a fresh
temporary directory, apply the patch there, then run those build/test commands.
The patch applies to this completed working tree, not directly to the old audit
commit. No hardware was flashed and no reset/relaunch behavior was tested.

## Decisions and staged migration

1. Preserve C interop. `picolibc`, the libc build recipe and generated header make
   it a supported boundary even though no C app currently lives in-tree.
2. Keep the current shared allocator plus exact ownership tracking for this
   release. Add an app byte budget/arena before promising OOM isolation or abort
   reclamation. A count limit alone does not reserve kernel memory.
3. Do not claim runtime PROC1 reset is safe merely because startup resets it.
   Use cooperative exit initially; retain whole-device reset as panic fallback.
4. Retain serialized framebuffer ownership and copied audio packets. Do not
   reintroduce shared mutable references or in-flight app-owned DMA buffers.
5. Make v2 an explicit clean ABI break. Reject v1 with on-screen rebuild guidance;
   support only documented append-only minor prefixes within v2.
6. Keep synchronous blocking sleep for the single app, with cancellation-aware
   bounded waits. Core0 remains asynchronous; no async ABI is needed yet.

Migration order: review descriptor and scalar ABI; finalize every slot signature
and short-write contract; implement descriptor retention and rejection UI;
convert wrappers/apps together; add cooperative cancellation and quotas; validate
on both boards. Require repeated launches, full keyboard queues, SD removal,
malformed ELF/GIF/WAV inputs, full audio queues, app panic, and cancellation at
every resource boundary on hardware before enabling recovery claims.

Decision point: approve the production v2 contract and lifecycle policy before
applying a migration. The prototype deliberately does not implement hard abort.

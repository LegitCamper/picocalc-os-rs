# Plan 003: Make the ELF loader reject malformed binaries instead of corrupting memory

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat b619dfa..HEAD -- kernel/src/elf.rs kernel/src/ui.rs userlib_sys/src/lib.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: plans/001-verification-baseline.md
- **Category**: security
- **Planned at**: commit `b619dfa`

## Why this matters

This is a bare-metal Rust OS for the Clockwork PicoCalc (RP2350, Cortex-M33,
no MMU, no memory protection). `kernel/src/elf.rs` loads a position-independent
ELF32 from the SD card into a `bumpalo::Bump` arena and runs it. Because there
is no MMU, **the loader is the only thing standing between a malformed `.bin`
file on the SD card and arbitrary kernel memory corruption** — a bad
`p_vaddr`, a bad relocation offset, or a bad symbol address writes straight
into kernel statics. Anyone who drops a file on the SD card is the attacker,
and the symptoms (random hangs, corrupted framebuffer, watchdog resets) look
exactly like the instability the README is chasing. Several of these are also
plain crashes on a *truncated but honest* file, which is the common case.

## Current state

`kernel/src/elf.rs` — ELF32 PIE loader. Entry point:

```rust
// kernel/src/elf.rs:34
pub async fn load_binary(name: &ShortFileName) -> Result<(EntryFn, Bump), LoadError> {
```

The error type already exists and is the right place to add variants:

```rust
// kernel/src/elf.rs:23-32
#[derive(Debug)]
pub enum LoadError {
    WrongMachine,
    InvalidElf,
    FailedToReadFile,
    ElfIsNotPie,
    UnknownRelocationType,
    SyscallTableNotFound,
    SyscallTableSizeMismatch,
}
```

A real app binary, for reference (`arm-none-eabi-readelf` on
`target/thumbv8m.main-none-eabihf/release-binary/calculator`): 7 program
headers, 18 sections, `min_vaddr` 0, entry `0x1261`, `.syscall_table` at
`0xbd80` size `0x44` (= 17 × 4), 75 `R_ARM_RELATIVE` relocations in `.rel.dyn`.
`user_apps/memory.x` places everything in one `RAM` region at origin `0x0`,
length `250K`.

The verified defects:

**1. Header read is not length-checked**

`kernel/src/elf.rs:38-45`:

```rust
let mut header_buf = [0; ELF32_HDR_SIZE];
sd.read_file(name, |mut file| {
    file.read(&mut header_buf)
        .map_err(|_| LoadError::FailedToReadFile)?;
    let elf_header = Header::from_bytes(&header_buf);
    validate_header(&elf_header)?;
```

`File::read` returns the number of bytes read; it is discarded. A file shorter
than 52 bytes is parsed from a partially-zeroed buffer. `validate_header`
(`:92-103`) also never checks `e_ident` — the ELF magic is never verified — nor
that `e_phentsize`/`e_shentsize` match the sizes the casts assume.

**2. Segment placement trusts `p_vaddr` and `p_memsz`**

`kernel/src/elf.rs:49-69`:

```rust
let (total_size, min_vaddr, _max_vaddr) =
    total_loadable_size(&mut file, elf_header, &mut ph_buf)?;

let bump = Bump::with_capacity(total_size);
let base = bump.alloc_slice_fill_default::<u8>(total_size);

for i in 0..elf_header.e_phnum {
    ...
    let seg_offset = (ph.p_vaddr - min_vaddr) as usize;
    let segment = &mut base[seg_offset..seg_offset + ph.p_memsz as usize];

    if ph.p_type == PT_LOAD {
        load_segment(&mut file, &ph, segment)?;
    }
}
```

Three problems in four lines. `ph.p_vaddr - min_vaddr` underflows for any
non-`PT_LOAD` header whose `p_vaddr` is below `min_vaddr` (and `min_vaddr` is
computed from `PT_LOAD` headers only, `:276-283`). The slice is built
**before** the `PT_LOAD` check, so a `DYNAMIC` or `GNU_STACK` header with a
large `p_vaddr` panics on the slice index. And `total_loadable_size`
(`:263-288`) computes `max_vaddr` with unchecked `ph.p_vaddr + ph.p_memsz` and
`total_size = max_vaddr - min_vaddr`, both of which can overflow or come out
absurdly large — `Bump::with_capacity` then calls `oom()`, which panics.

**3. Relocation offsets are unchecked**

`kernel/src/elf.rs:137-172`:

```rust
let num_relocs = sh.sh_size as usize / sh.sh_entsize as usize;
...
let reloc_addr = unsafe { base.add((rel.r_offset - min_vaddr) as usize) as *mut u32 };

match reloc_type {
    R_ARM_RELATIVE => {
        unsafe {
            let val = ptr::read_unaligned(reloc_addr);
            ptr::write_unaligned(reloc_addr, val.wrapping_add(base as u32));
        }
    }
```

`sh_entsize` is attacker-controlled and `== 0` divides by zero. `r_offset` is
never bounds-checked against the arena, and `r_offset - min_vaddr` underflows.
The `write_unaligned` then writes 4 bytes anywhere in the address space. This
is the single most dangerous line in the repo. Note the loop also hardcodes an
8-byte stride (`:148`) while using `sh_entsize` for the count — those must
agree.

**4. Symbol-table walk is unchecked and O(n²) in file reads**

`kernel/src/elf.rs:174-260` (`patch_syscalls`):

- `sh.sh_size as usize / sh.sh_entsize as usize` (`:192`) — same division by
  zero.
- `vec![0u8; sh.sh_size as usize]` (`:185`) — an attacker-chosen allocation
  from the kernel's 250 KiB arena; no cap.
- `.expect("symbol was not utf8")` (`:214`) — panics the kernel on a
  non-UTF-8 symbol name.
- The name is read **one byte at a time** with a `seek` + `read` syscall pair
  per byte (`:204-212`), and `read_section(file, elf_header, sh.sh_link)` is
  re-read **inside the per-symbol loop** (`:198`). For the calculator's 371
  symbols this is thousands of SD reads per launch.
- `base.add((sym.st_value as usize) - min_vaddr as usize)` (`:225-227`) —
  unchecked, and `sym.st_value` is attacker-controlled. The size check on the
  line above it (`:220-223`) is correct and must stay.

**5. Entry point is unchecked**

`kernel/src/elf.rs:82-84`:

```rust
let entry_ptr: EntryFn = unsafe {
    core::mem::transmute(base.as_ptr().add((elf_header.e_entry - min_vaddr) as usize))
};
```

`e_entry` is never validated to be inside the arena, and the subtraction
underflows if `e_entry < min_vaddr`. Note that on ARM a Thumb entry point also
needs its low bit set; the real binaries work because `_start` is reached via
a `blx`-equivalent call through the `fn()` pointer, so do **not** change the
transmute's semantics — only bound-check the offset.

**6. A load failure panics the kernel instead of showing an error**

`kernel/src/ui.rs:46-52`:

```rust
let entry = unsafe {
    match load_binary(&selection.short_name).await {
        Ok(entry) => entry,
        Err(e) => panic!("unable to load binary: {:?}", e),
    }
};
BINARY_CH.send(entry).await;
```

All the loader's careful `Result` plumbing ends in a `panic!`. Picking a
corrupt file from the menu resets the device.

### Conventions to match

- The loader returns `Result<_, LoadError>`; add variants rather than
  panicking. Exemplar: `validate_header` at `kernel/src/elf.rs:92-103`.
- Logging: `#[cfg(feature = "defmt")] defmt::warn!(...)`, `_`-prefixed
  locals. Exemplar `kernel/src/syscalls.rs:63-74`.
- `// SAFETY:` comment above every `unsafe` block.
- Pure logic that should be unit-tested goes in `userlib_sys` (it is the only
  host-testable crate; see `plans/001-verification-baseline.md`).
- UI text drawing exemplar for step 6: `kernel/src/ui.rs:82-99` draws a
  centered `TextBox` into `FRAMEBUFFER` for the "no binaries" case.

## Commands you will need

| Purpose       | Command                                                                 | Expected on success |
|---------------|-------------------------------------------------------------------------|---------------------|
| Host tests    | `just test`                                                             | exit 0, tests pass  |
| Kernel builds | `cargo build --bin kernel --release --no-default-features --features rp235xa` | exit 0        |
| Apps build    | `just userapps`                                                         | exit 0              |
| Full gate     | `just check`                                                            | exit 0              |
| Inspect a real binary | `arm-none-eabi-readelf -h -l -S target/thumbv8m.main-none-eabihf/release-binary/calculator` | prints headers |

## Scope

**In scope**:
- `kernel/src/elf.rs`
- `userlib_sys/src/lib.rs` — only to add the `ElfBounds` helper and its host
  tests (step 1).
- `kernel/src/ui.rs` — only the `panic!` at line 49 (step 6).

**Out of scope** (do NOT touch):
- `kernel/src/syscalls.rs` — plan 002 owns it.
- `user_apps/memory.x` and the per-app `build.rs` files — the link layout is
  correct and changing it invalidates every existing `.bin` on users' cards.
- The `SyscallTable` enum, its order, and `SYS_CALL_TABLE_COUNT` — this plan
  must not change the ABI.
- Adding a new crate (e.g. a different ELF parser). `goblin` with
  `default-features = false, features = ["elf32"]` is already the dependency;
  keep it.

## Git workflow

- Branch: `advisor/003-elf-loader-hardening`
- Commit per step; short imperative lowercase messages matching `git log`.
- Do NOT push or open a PR unless the operator asks.

## Steps

### Step 1: Add a tested bounds helper to `userlib_sys`

The same "is this offset inside the arena" question is asked for segments,
relocations, symbols and the entry point. Write it once, test it on the host.

Add to `userlib_sys/src/lib.rs`:

```rust
/// Bounds of a loaded image: the `[min_vaddr, min_vaddr + size)` window that
/// every virtual address in the ELF must fall inside.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ElfBounds {
    pub min_vaddr: u32,
    pub size: usize,
}

impl ElfBounds {
    /// Converts a virtual address to an offset into the loaded image,
    /// returning `None` if `vaddr` is below the image or if `[vaddr, vaddr+len)`
    /// does not fit entirely inside it.
    pub fn offset_of(&self, vaddr: u32, len: usize) -> Option<usize> {
        let offset = vaddr.checked_sub(self.min_vaddr)? as usize;
        let end = offset.checked_add(len)?;
        (end <= self.size).then_some(offset)
    }
}
```

Host tests, in the `mod tests` block created by plan 001:

```rust
#[test]
fn offset_of_accepts_address_at_image_start() {
    let b = ElfBounds { min_vaddr: 0x1000, size: 64 };
    assert_eq!(b.offset_of(0x1000, 4), Some(0));
}

#[test]
fn offset_of_accepts_last_fitting_window() {
    let b = ElfBounds { min_vaddr: 0x1000, size: 64 };
    assert_eq!(b.offset_of(0x103c, 4), Some(60));
}

#[test]
fn offset_of_rejects_address_below_image() {
    let b = ElfBounds { min_vaddr: 0x1000, size: 64 };
    assert_eq!(b.offset_of(0x0fff, 4), None);
}

#[test]
fn offset_of_rejects_window_past_image_end() {
    let b = ElfBounds { min_vaddr: 0x1000, size: 64 };
    assert_eq!(b.offset_of(0x103d, 4), None);
}

#[test]
fn offset_of_rejects_length_overflow() {
    let b = ElfBounds { min_vaddr: 0, size: 64 };
    assert_eq!(b.offset_of(0, usize::MAX), None);
}
```

**Verify**: `just test` → exit 0, 5 new tests pass.

### Step 2: Validate the ELF header properly

Add these variants to `LoadError` (`kernel/src/elf.rs:23-32`):
`BadMagic`, `TruncatedFile`, `BadHeaderSizes`, `ImageTooLarge`,
`AddressOutOfBounds`, `BadSectionEntrySize`, `BadSymbolName`.

In `load_binary` (`:38-45`), check the read length:

```rust
let read = file
    .read(&mut header_buf)
    .map_err(|_| LoadError::FailedToReadFile)?;
if read != ELF32_HDR_SIZE {
    return Err(LoadError::TruncatedFile);
}
```

Extend `validate_header` (`:92-103`) — keep the three existing checks and add:

- ELF magic: `h.e_ident[0..4] == [0x7f, b'E', b'L', b'F']` else `BadMagic`.
- `h.e_phentsize as usize == core::mem::size_of::<ProgramHeader>()` and
  `h.e_shentsize as usize == core::mem::size_of::<SectionHeader>()` else
  `BadHeaderSizes`. (For the real binaries these are 32 and 40 — confirm with
  the `readelf` command in the table above.)
- `h.e_phnum > 0` else `InvalidElf`.

**Verify**: `cargo build --bin kernel --release --no-default-features --features rp235xa`
→ exit 0.

### Step 3: Bound the image size and fix segment placement

In `total_loadable_size` (`:263-288`):

- Use `checked_add` for `ph.p_vaddr + ph.p_memsz`; on overflow return
  `LoadError::ImageTooLarge`.
- Return `LoadError::InvalidElf` if no `PT_LOAD` header was found (currently
  `min_vaddr` stays `u32::MAX` and `max_vaddr - min_vaddr` underflows).
- Compute `total_size` with `checked_sub`, and reject sizes above a new
  constant. `user_apps/memory.x` gives apps a 250 KiB region, and the kernel
  arena is 250 KiB (`kernel/src/main.rs:96`) / 400 KiB with `psram`
  (`:98`), so:

```rust
/// Largest image the loader will attempt. Matches the `RAM` region in
/// `user_apps/memory.x`; a larger image cannot have been linked by this repo
/// and would exhaust the kernel arena.
const MAX_IMAGE_SIZE: usize = 250 * 1024;
```

  Reject `total_size > MAX_IMAGE_SIZE` with `LoadError::ImageTooLarge`. Use
  `Bump::try_with_capacity(total_size).map_err(|_| LoadError::ImageTooLarge)?`
  instead of `Bump::with_capacity` (`:52`), which panics via `oom()`.

In the segment loop (`:56-69`), move the `PT_LOAD` check **before** the slice
is created, and use the step-1 helper:

```rust
let bounds = ElfBounds { min_vaddr, size: total_size };

for i in 0..elf_header.e_phnum {
    // ... seek + read ph_buf as today ...
    let ph = cast_phdr(&ph_buf);

    if ph.p_type != PT_LOAD {
        continue;
    }

    let seg_offset = bounds
        .offset_of(ph.p_vaddr, ph.p_memsz as usize)
        .ok_or(LoadError::AddressOutOfBounds)?;
    let segment = &mut base[seg_offset..seg_offset + ph.p_memsz as usize];
    load_segment(&mut file, &ph, segment)?;
}
```

Also in `load_segment` (`:105-135`): return `LoadError::InvalidElf` if
`ph.p_filesz > ph.p_memsz` (otherwise `segment[dst_offset..dst_offset+to_read]`
panics), and check the per-chunk read length like step 2 does, returning
`TruncatedFile` on a short read.

**Verify**: `just userapps && cargo build --bin kernel --release --no-default-features --features rp235xa`
→ both exit 0.

### Step 4: Bound every relocation write

Rewrite the body of `apply_relocations` (`:137-172`) to take `bounds` instead
of `min_vaddr`, and:

- Return `LoadError::BadSectionEntrySize` if
  `sh.sh_entsize as usize != core::mem::size_of::<Rel>()` (8). This removes
  both the division by zero and the stride/count mismatch at `:145-148`.
- Replace the unchecked pointer arithmetic with:

```rust
let offset = bounds
    .offset_of(rel.r_offset, core::mem::size_of::<u32>())
    .ok_or(LoadError::AddressOutOfBounds)?;
// SAFETY: `offset_of` guarantees `[offset, offset+4)` is inside the
// `total_size` bytes allocated at `base`.
let reloc_addr = unsafe { base.add(offset) as *mut u32 };
```

Keep the `R_ARM_RELATIVE` arm and its `read_unaligned`/`write_unaligned` pair
exactly as it is, and keep returning `UnknownRelocationType` for anything
else. The real binaries use only `R_ARM_RELATIVE` (75 of them in
`calculator`), so no new relocation types are needed.

Update the call site at `:74-76` to pass `bounds`.

**Verify**: `just userapps` → exit 0, then flash and launch an app (manual —
see "Done criteria"). `cargo build --bin kernel --release --no-default-features --features rp235xa`
→ exit 0.

### Step 5: Fix `patch_syscalls` — bounds, no panic, and stop the per-byte reads

Rewrite `patch_syscalls` (`:174-260`) keeping its contract
(`Ok(())` once the `SYS_CALL_TABLE` symbol is found and patched,
`Err(LoadError::SyscallTableNotFound)` otherwise) and keeping the existing
size check at `:220-223` verbatim. Changes:

1. `sh.sh_entsize as usize != core::mem::size_of::<Sym>()` (16) →
   `LoadError::BadSectionEntrySize`. Removes the division by zero at `:192`.
2. Cap the symtab allocation: reject `sh.sh_size as usize > MAX_IMAGE_SIZE`
   with `LoadError::ImageTooLarge`, and check the `file.read` length like
   step 2.
3. Hoist `read_section(file, elf_header, sh.sh_link)` **out** of the
   per-symbol loop (it is loop-invariant; currently re-read for all 371
   symbols).
4. Read the string table **once** into a `Vec<u8>` (same size cap and
   length check) and slice the name out of it instead of the per-byte
   `seek`+`read` loop at `:204-212`. Find the terminator with
   `iter().position(|&b| b == 0)`.
5. Replace `.expect("symbol was not utf8")` (`:214`) with
   `core::str::from_utf8(..).map_err(|_| LoadError::BadSymbolName)?` — or skip
   the symbol; either is acceptable, but it must not panic.
6. Bound the table write with the step-1 helper:

```rust
let table_offset = bounds
    .offset_of(sym.st_value, expected_size)
    .ok_or(LoadError::AddressOutOfBounds)?;
// SAFETY: `offset_of` guarantees the whole table fits inside the image,
// and `expected_size` was checked against `sym.st_size` above.
let table_base = unsafe { base.add(table_offset) } as *mut usize;
```

Leave the `SyscallTable::iter()` match arms (`:229-250`) untouched — the
syscall order is the ABI.

**Verify**: `cargo build --bin kernel --release --no-default-features --features rp235xa`
→ exit 0. `grep -c 'expect(' kernel/src/elf.rs` → `0`.

### Step 6: Bound the entry point, and stop panicking on load failure

In `load_binary` (`:82-84`):

```rust
let entry_offset = bounds
    .offset_of(elf_header.e_entry, 2)
    .ok_or(LoadError::AddressOutOfBounds)?;
// SAFETY: `offset_of` guarantees the entry address is inside the loaded
// image. The ELF is a PIE whose entry is `_start`, declared `extern "Rust"
// fn()` — see `user_apps/*/src/main.rs`.
let entry_ptr: EntryFn = unsafe { core::mem::transmute(base.as_ptr().add(entry_offset)) };
```

(The `2` is the minimum size of a Thumb instruction.)

In `kernel/src/ui.rs:46-52`, replace the `panic!` with a non-fatal path:

```rust
let entry = match unsafe { load_binary(&selection.short_name).await } {
    Ok(entry) => entry,
    Err(_e) => {
        #[cfg(feature = "defmt")]
        defmt::error!("unable to load binary: {:?}", _e);
        continue;
    }
};
BINARY_CH.send(entry).await;
```

`LoadError` must derive `defmt::Format` behind `#[cfg(feature = "defmt")]` for
that log to compile, or format it with `{}`/`Debug2Format`. Verify which the
repo already does elsewhere before choosing; `kernel/src/elf.rs:23` currently
derives only `Debug`.

The `continue` keeps the kernel UI loop alive so the user can pick another
file. Drawing an on-screen error message would be nicer but needs the
`FRAMEBUFFER` pattern from `kernel/src/ui.rs:82-99`; it is explicitly
deferred (see maintenance notes).

**Verify**: `grep -c 'panic!' kernel/src/ui.rs` → `0`. `just check` → exit 0.

### Step 7: Confirm real binaries still load

**Verify**: `just userapps` → exit 0.
`arm-none-eabi-readelf -h target/thumbv8m.main-none-eabihf/release-binary/calculator | grep -E 'Size of (program|section) headers'`
→ 32 and 40, matching the `e_phentsize`/`e_shentsize` checks added in step 2.
`just test` → exit 0.

## Test plan

- New tests: the five `ElfBounds::offset_of` cases from step 1, in
  `userlib_sys/src/lib.rs`'s `mod tests` — image start, last fitting window,
  below-image rejection, past-end rejection, length overflow. These are the
  logic that prevents the out-of-bounds writes; they are the part that can be
  tested without hardware.
- The loader itself cannot be unit-tested on the host (it is built around
  `embedded_sdmmc::File` and `embassy` locks). Its gate is: compiles, and the
  five real app binaries still load on device.
- Model after the test block plan 001 creates in the same file.
- Verification: `just test` → exit 0, 5 new tests pass.

## Done criteria

ALL must hold:

- [ ] `just test` exits 0; the 5 `ElfBounds` tests exist and pass
- [ ] `just check` exits 0
- [ ] `grep -c 'expect(' kernel/src/elf.rs` → 0
- [ ] `grep -c 'panic!' kernel/src/ui.rs` → 0
- [ ] `grep -c 'Bump::with_capacity' kernel/src/elf.rs` → 0 (replaced by
      `try_with_capacity`)
- [ ] Every `base.add(...)` in `kernel/src/elf.rs` is preceded by an
      `offset_of` call and a `// SAFETY:` comment
- [ ] `grep -c 'sh_size as usize / sh.sh_entsize as usize' kernel/src/elf.rs`
      → 0 (entsize is validated before any division)
- [ ] `SYS_CALL_TABLE_COUNT` is still `17` and the `SyscallTable` match arms
      are unchanged
- [ ] `git status` shows only `kernel/src/elf.rs`, `kernel/src/ui.rs`,
      `userlib_sys/src/lib.rs` modified
- [ ] `plans/README.md` status row updated

**Manual device check** (operator; record as pending if no hardware):
`just kernel-release rp235xa`, `just copy-userapps`, then launch each of the
five apps from the menu. Expected: all five start as before. Then copy a
deliberately corrupt file — `head -c 4096 /dev/urandom > junk.bin` — onto the
card and select it. Expected: an error is logged and the menu stays usable; no
reset.

## STOP conditions

- `goblin`'s `header32::Header` does not expose `e_ident` as an array — report
  the actual field shape rather than skipping the magic check.
- A real app binary fails any of the new validations (step 7 fails). That
  means a bound is wrong, not that the binary is bad — stop and report which
  check rejected it and with what values.
- `Bump::try_with_capacity` is unavailable in the pinned `bumpalo` 3.19 —
  report instead of reintroducing the panicking constructor.
- Fixing a bound appears to require changing `user_apps/memory.x` or the link
  arguments. It does not; stop and report.

## Maintenance notes

- `MAX_IMAGE_SIZE` is tied to the `RAM` length in `user_apps/memory.x`
  (250 KiB) and to the kernel arena in `kernel/src/main.rs:96-98`. If either
  changes, this constant must change with it — add a comment pointing both
  ways.
- The loader still only supports `R_ARM_RELATIVE`. That is correct for
  everything this repo links today (verified: 75/75 relocations in
  `calculator`), but C code linked against picolibc may emit `R_ARM_ABS32` or
  GOT-relative types; adding one means adding an arm, not loosening the
  bounds.
- Deferred on purpose: drawing the load error on screen instead of only
  logging it (needs the `FRAMEBUFFER` text path from `kernel/src/ui.rs:82-99`),
  and verifying the Thumb bit on `e_entry`.
- Reviewer should scrutinize: that no `offset_of` result is used after being
  recomputed by hand, and that the `SAFETY` comments name the specific
  invariant the check established.

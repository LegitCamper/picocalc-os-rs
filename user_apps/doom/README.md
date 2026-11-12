# Doom for PicoCalc

Single-player, silent Doom port targeting **pimoroni2w with 8 MiB XIP PSRAM**.
Builds a Cortex-M33 hard-float PIE ELF with the kernel's 18-entry syscall ABI,
including `get_last_error` status reporting and the out-pointer `get_key`.
Cross-linking, loader compatibility checks, and host bridge tests pass. Gameplay
and performance on physical hardware have not been verified.

## Build and run

Requirements: the repository's Rust toolchain (for `rust-lld`), Python 3,
`arm-none-eabi-gcc`/binutils, and a **PIC** picolibc build. Host checks also need
a C compiler with AddressSanitizer and UndefinedBehaviorSanitizer.

```sh
git submodule update --init picolibc user_apps/doom/doomgeneric
just newlib
just doom
./user_apps/doom/sizes.sh
cargo build -p kernel --release --no-default-features --features pimoroni2w
```

`PICOLIBC` may point to an existing PIC picolibc install prefix containing
`include/` and `lib/libc.a`. The default is
`picolibc/build/install/usr/local/picolibc/thumbv8m_main_fp-none-eabi`.
The existing `just newlib` recipe configures `-Db_pie=true` and
`-Dtinystdio=false`; do not substitute the toolchain's non-PIC libc.

Flash the rebuilt kernel using the normal board workflow. Put
`target/doom/doom.bin` and your own **trusted Doom shareware IWAD**, named
`doom1.wad`, in the SD card root, then launch `doom.bin` from the kernel menu.
No game data is included or downloaded. Directory/lump ranges are checked before
startup, but this does not make the legacy engine safe for malicious WAD contents.
Other IWADs, PWADs, and arbitrary paths are not supported by this initial port.
Do not use `objcopy -O binary` or strip all symbols: the kernel needs ELF sections
and the `SYS_CALL_TABLE` symbol. The build strips debug data only.

## Memory

Measured with ARM GCC 16.2.0 and the existing PIC picolibc build:

| Resource | Bytes |
| --- | ---: |
| Loaded ELF span, including BSS and relocations | 520,176 |
| Bounded libc heap, including Doom's 2 MiB zone | 3,145,728 |
| Existing kernel framebuffer | 204,800 |
| Combined allocation budget, excluding allocator metadata | 3,870,704 |

The build reads the kernel's PSRAM `MAX_IMAGE_SIZE` ceiling and checks a 4 MiB
image/heap/framebuffer budget, so a kernel-side change cannot silently diverge.
Targeting 8 MiB leaves headroom for kernel allocations and metadata.
Actual numbers vary with compiler/libc versions; `build.py` reports and checks
the linked artifact, not a sum of unlinked object sizes.

Savings: zone reduced from 6 MiB to 2 MiB; direct access to Doom's indexed
320x200 framebuffer removes DoomGeneric's extra 256,000-byte RGB buffer;
four-row RGB565 blits need only 3 KiB of conversion scratch. Eight single-player
backup tics, fixed display lookup sizes, and 16-bit `viewangletox` reduce static
storage without reducing vanilla renderer limits.

The original 352 KiB object-only experiment cut visplanes, openings, drawsegs,
and sprites. Those aggressive limits are **not enabled** in this build: they can
break maps or overrun renderer buffers. Default renderer capacities remain.
The shrink patch still permits overrides for experiments, not supported gameplay.

The kernel loader owns one aligned, zero-filled image allocation per launch and
frees it on load failure or app return. With `psram` the image lives in the XIP
window and `MAX_IMAGE_SIZE` is 640 KiB; without it, images stay in the SRAM
arena under the original 250 KiB ceiling. Merely widening a linker window would
not move an image out of SRAM. PSRAM exhaustion returns null rather than
panicking; Doom reports a failed private-heap allocation and returns without
starting the engine. Syscall reads are checked with `get_last_error`, so a
rejected read is an error instead of apparent zeroed WAD data.
The shared `user_apps/memory.x` is unchanged; Doom has its own linker script.

## Controls and limits

- Arrows: move/turn; Z or Ctrl: fire; Space: use.
- Comma/period: strafe; Shift: run; Alt: strafe modifier.
- Enter: confirm; Escape: menu; Tab: automap; number keys: weapons.
- F1-F10 map to Doom's function keys. Break returns immediately to the kernel.
- Display: 320x200, centered vertically in the 320x320 panel.
- No sound, music, networking, saves, screenshots, or persistent settings.
  Save requests show a read-only notice. Unsupported file operations return
  errors rather than pretending data was saved. The kernel has no checked
  truncate/rename/write-result interface for safe save replacement yet.
- Exit and fatal engine errors return through a C `setjmp` boundary; the entire
    private heap is freed. The platform patch makes the engine's normal quit path
    call `exit` after cleanup instead of returning to its game loop.
    Each launch requires a freshly loaded ELF, as the kernel
  already provides. Missing/invalid WAD diagnostics use the kernel print syscall;
  enable the kernel's `defmt` feature to see them.

## Checks

`sizes.sh` builds the actual app and runs `check.py`. Builds patch temporary
copies, never the submodule worktree. `check.py` tests read/seek/EOF/error paths,
read-only writes, WAD bounds, heap exhaustion, keyboard events, RGB565 output,
timing, allocation failure, and cleanup under host sanitizers. The shutdown check
links the patched engine's actual `I_Quit`/`I_AtExit`, not a simulated engine exit.
It also checks ARM/Thumb/hard-float PIE,
segment budget, no imports/TLS/interpreter, only `R_ARM_RELATIVE` relocations,
and the 18-slot syscall table, then simulates relocation and syscall-table
patching. `just check` additionally runs the kernel's loader-contract test.

Host bridge tests do not execute Doom gameplay or prove SD/display timing on
the device. Hardware boot, level transitions, longer play and repeated launches
remain necessary before calling this a working release.

Upstream engine source and its license remain in the pinned `doomgeneric`
submodule; local changes live in the two patch files.

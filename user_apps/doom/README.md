# Doom for PicoCalc

Single-player Doom port with stereo sound effects targeting
**pimoroni2w with 8 MiB XIP PSRAM**.
Builds a Cortex-M33 hard-float PIE ELF with the kernel's 18-entry syscall ABI,
including `get_last_error` status reporting and the out-pointer `get_key`.
Cross-linking, loader compatibility checks, and host bridge tests cover the port.
Display and framerate have user-reported hardware confirmation; updated keyboard
delivery and audio still need on-device verification.

## Build and run

Requirements: the repository's Rust toolchain (for `rust-lld`), Python 3,
`arm-none-eabi-gcc`/binutils, and a **PIC** picolibc build. Host checks also need
a C compiler with AddressSanitizer and UndefinedBehaviorSanitizer.

```sh
git submodule update --init user_apps/picolibc user_apps/doom/doomgeneric
just newlib
just doom
./user_apps/doom/sizes.sh
cargo build -p kernel --release --no-default-features --features pimoroni2w
```

`PICOLIBC` may point to an existing PIC picolibc install prefix containing
`include/` and `lib/libc.a`. The default is
`user_apps/picolibc/build/install/usr/local/picolibc/thumbv8m_main_fp-none-eabi`.
The existing `just newlib` recipe configures `-Db_pie=true` and
`-Dtinystdio=false`; do not substitute the toolchain's non-PIC libc.

Flash the rebuilt kernel using the normal board workflow. Put
`target/doom/doom.bin` and your own **trusted Doom shareware IWAD**, named
`doom1.wad`, in the SD card root, then launch `doom.bin` from the kernel menu.
No game data is included or downloaded. Directory/lump ranges are checked before
startup, but this does not make the legacy engine safe for malicious WAD contents.
Other IWADs, PWADs, and arbitrary paths are not supported by this initial port.
`just userapps` does not build Doom: use `just doom`, then replace `DOOM.BIN`
with `target/doom/doom.bin`. Do not copy an older binary from `release-binary/`.
Do not use `objcopy -O binary` or strip all symbols: the kernel needs ELF sections
and the `SYS_CALL_TABLE` symbol. The build strips debug data only.

## Memory

Measured with ARM GCC 16.2.0 and the existing PIC picolibc build:

| Resource | Bytes |
| --- | ---: |
| Loaded ELF span, including BSS and relocations | 521,944 |
| Bounded libc heap, including Doom's 2 MiB zone | 3,145,728 |
| Existing kernel framebuffer | 204,800 |
| Combined allocation budget, excluding allocator metadata | 3,872,472 |

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
frees it on load failure or app return. Images up to 250 KiB stay in SRAM,
including on PSRAM boards, so Rust apps can use exclusive atomics such as the
display singleton's compare-exchange. Larger images require `psram` and live in
the XIP window under the 640 KiB `MAX_IMAGE_SIZE` ceiling; those apps must not
use exclusive atomics in their image storage. Doom uses this larger-image path.
An SRAM allocation failure does not silently move an ordinary app to PSRAM.
Merely widening a linker window would
not move an image out of SRAM. PSRAM exhaustion returns null rather than
panicking; Doom reports a failed private-heap allocation and returns without
starting the engine. Syscall reads are checked with `get_last_error`, so a
rejected read is an error instead of apparent zeroed WAD data.
The shared `user_apps/memory.x` is unchanged; Doom has its own linker script.

## How to play

From the title screen, press **M** to open the menu. Select **New Game** with
Up/Down and press **Enter**. Choose the shareware episode and difficulty,
confirming each with Enter. **Escape quits the app**, not just the menu.

| Action | Keys |
| --- | --- |
| Move forward / backward | Up / Down or W / S |
| Turn left / right | Left / Right or A / D |
| Strafe left / right | Q / E or comma / period |
| Fire | Z or Ctrl |
| Use: open doors, activate switches | Space |
| Run | Hold Shift while moving |
| Strafe modifier | Hold Alt with Left / Right |
| Choose weapon | Number keys 1-7; weapon must be owned |
| Automap | Tab |
| Open / close menu, cancel | M |
| Menu navigation / confirm | Arrows / Enter; W/S/A/D also navigate |
| Menu back | Backspace |
| Quit to kernel | Escape or Break, including from menus |

Joystick direction events also move/turn; center confirms. Auxiliary left/right
button events fire/use; second left/right button events strafe. These aliases
only apply to hardware or firmware that emits those events. F1-F10 retain Doom's
function-key actions; F4 opens sound options. Save/load actions are unsupported.

Keyboard ABI uses three bytes in order: key, state, modifiers. State is explicitly
8-bit in both Rust and C. The old C bridge assumed a 12-byte event and read state
at offset 4 instead of offset 1, making every received key appear idle.

## Audio and limits

- Eight simultaneous WAD PCM sound effects, mixed into unsigned 8-bit stereo at
  22,050 Hz through the existing kernel audio API. Gunshots, doors, enemies, and
  menu effects are enabled; adjust SFX volume through F4 / Sound Options.
- Music is disabled with `-nomusic`; MUS/MIDI synthesis is not implemented.
- Mixing respects kernel backpressure without blocking gameplay. First-use
  effects load from SD; slow frames or SD reads can cause audio gaps.
- Display: 320x200, centered vertically in the 320x320 panel.
- No networking, saves, screenshots, or persistent settings.
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
read-only writes, WAD bounds, heap exhaustion, raw three-byte keyboard events
through Doom's actual event queue, RGB565 output, timing, allocation failure,
and cleanup under host sanitizers. Audio checks exercise the engine dispatcher,
PCM validation, stereo mixing, resampling, clipping, shared sample lifetime,
backpressure, rejected submissions, and shutdown. The shutdown check
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

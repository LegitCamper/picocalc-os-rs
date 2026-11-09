# doom (port groundwork)

`doomgeneric` vendored as a submodule, plus the shrink needed to make it
plausible on this kernel. No picocalc backend yet.

Run `./sizes.sh` to apply `shrink-tunables.patch` and measure the static
footprint (cortex-m33, `-Os`, SDL/allegro/statdump/sha1/i_scale excluded).

## Measured

| build | text+data | bss | total |
|---|---|---|---|
| stock, 320x200 | 272 KB | 237 KB | 509 KB |
| shrunk | 266 KB | 87 KB | 352 KB |

Shrinks (all `#ifndef`-guarded in the patch, values passed via `-D`):
`MAXVISPLANES` 128->24, `MAXOPENINGS` /4, `MAXDRAWSEGS` 256->64,
`MAXVISSPRITES` 128->32, `BACKUPTICS` 128->8 (single player only),
`MAXWIDTH`/`MAXHEIGHT` 1120x832 -> 320x200, zone `DEFAULT_RAM` 6 MB -> 2 MB,
and `viewangletox` `int`->`short` (holds 0..320).

## Blocker

352 KB static + ~2 MB zone heap does not fit a non-PSRAM rp2350:

- `user_apps/memory.x` caps apps at `250K`; needs ~512K.
- `kernel/src/elf.rs` bump-loads every segment (text and rodata included)
  into the kernel heap -- 250 KB arena, 200 KB of which is the framebuffer.

Viable target is `pimoroni2w` (8 MB XIP PSRAM, 400 KB arena). Fitting the
plain rp2350 needs XIP-from-flash app text and WAD streaming off SD, i.e.
rp2040-doom-scale work.

## Next

Write `doomgeneric_picocalc.c` against `userlib_sys.h`: `DG_Init`,
`DG_DrawFrame` (palette index -> Rgb565 via `blit`), `DG_GetKey`
(`get_key`), `DG_GetTicksMs` (`get_ms`), `DG_SleepMs` (`sleep`), and
`I_ZoneBase` over `alloc`. WAD reads go through `read_file`/`file_len`.

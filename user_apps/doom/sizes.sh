#!/usr/bin/env bash
# Measures static footprint of doomgeneric shrunk for picocalc, and asserts
# it stays under the budget a PSRAM-backed app load can satisfy.
#
# ponytail: measurement only -- no picocalc syscall backend yet. The numbers
# below are what any future doomgeneric_picocalc.c must live inside.
set -euo pipefail

cd "$(dirname "$0")"
SRC=doomgeneric/doomgeneric
OUT=${OUT:-target-sizes}

git -C doomgeneric diff --quiet -- . || { echo "submodule dirty; patch already applied?" >&2; exit 1; }
git -C doomgeneric apply ../shrink-tunables.patch
trap 'git -C doomgeneric checkout -- .' EXIT

mkdir -p "$OUT"

# 320x200 doom framebuffer letterboxed into the 320x320 picocalc panel.
DEFS=(
  -DDOOMGENERIC_RESX=320 -DDOOMGENERIC_RESY=200
  -DMAXWIDTH=320 -DMAXHEIGHT=200
  -DMAXVISPLANES=24 -DMAXOPENINGS='SCREENWIDTH*16'
  -DMAXDRAWSEGS=64 -DMAXVISSPRITES=32
  -DBACKUPTICS=8            # single player: no netgame tic backlog
  -DDEFAULT_RAM=2 -DMIN_RAM=2
  -DNORMALUNIX -DDOOMGENERIC
)
CFLAGS=(
  -c -Os -mcpu=cortex-m33 -mthumb -mfloat-abi=hard -mfpu=fpv5-sp-d16
  -ffunction-sections -fdata-sections -fno-strict-aliasing -I"$SRC"
)

for f in "$SRC"/*.c; do
  case $(basename "$f") in
    doomgeneric_*|i_main*|i_allegro*|i_sdl*|statdump.c|sha1.c|i_scale.c) continue ;;
  esac
  arm-none-eabi-gcc "${CFLAGS[@]}" "${DEFS[@]}" -o "$OUT/$(basename "${f%.c}").o" "$f"
done

read -r text data bss _ < <(arm-none-eabi-size -t "$OUT"/*.o | tail -1)
echo "text=$text data=$data bss=$bss total=$((text + data + bss))"

# Budget: 512K app RAM window (user_apps/memory.x must be raised from 250K),
# leaving room for stack and the zone heap allocated on top.
limit=$((360 * 1024))
total=$((text + data + bss))
[ "$total" -lt "$limit" ] || { echo "FAIL: $total >= $limit" >&2; exit 1; }
echo "OK: static footprint under $((limit / 1024))K"

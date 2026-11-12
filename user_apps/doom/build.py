#!/usr/bin/env python3
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from check import check_elf

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent.parent
OUT = REPO / "target" / "doom"
CC = os.environ.get("CC_ARM", "arm-none-eabi-gcc")
PICOLIBC = Path(os.environ.get("PICOLIBC", str(
    REPO / "user_apps/picolibc/build/install/usr/local/picolibc/thumbv8m_main_fp-none-eabi")))
FLAGS = ["-Os", "-mcpu=cortex-m33", "-mthumb", "-mfloat-abi=hard", "-mfpu=fpv5-sp-d16",
         "-fPIE", "-ffunction-sections", "-fdata-sections", "-fno-strict-aliasing",
         "-fno-stack-protector", "-std=gnu11", "-D_DEFAULT_SOURCE", "-DNORMALUNIX",
         "-DPICOCALC", "-DCMAP256", "-DDOOMGENERIC_RESX=320", "-DDOOMGENERIC_RESY=200",
         "-DMAXWIDTH=320", "-DMAXHEIGHT=200", "-DBACKUPTICS=8",
         "-DDEFAULT_RAM=2", "-DMIN_RAM=2", "-D_FILE_OFFSET_BITS=64"]


def run(*args, **kwargs):
    subprocess.run(args, check=True, **kwargs)


def main():
    if not (PICOLIBC / "lib/libc.a").is_file():
        raise SystemExit("Build PIC picolibc with `just newlib`, or set PICOLIBC to its install prefix.")
    upstream = ROOT / "doomgeneric/doomgeneric"
    if not (upstream / "Makefile").is_file():
        raise SystemExit("Run `git submodule update --init user_apps/doom/doomgeneric` first.")
    OUT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="picocalc-doom-") as temporary:
        staging = Path(temporary)
        source = staging / "doomgeneric"
        shutil.copytree(upstream, source)
        run("git", "apply", str(ROOT / "shrink-tunables.patch"), cwd=staging)
        run("git", "apply", str(ROOT / "picocalc.patch"), cwd=staging)
        names = next(line.split("=", 1)[1].split() for line in
                     (source / "Makefile").read_text().splitlines() if line.startswith("SRC_DOOM ="))
        sources = [source / name.replace(".o", ".c") for name in names
                   if name not in ("doomgeneric.o", "doomgeneric_xlib.o")]
        sources.append(ROOT / "picocalc.c")
        objects = []
        for path in sources:
            target = staging / (path.stem + ".o")
            run(CC, *FLAGS, "-isystem", str(PICOLIBC / "include"), "-I", str(source),
                "-c", str(path), "-o", str(target))
            objects.append(str(target))
        sysroot = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
        linker = next(Path(sysroot).glob("lib/rustlib/*/bin/rust-lld"))
        libgcc = subprocess.check_output([CC, *FLAGS[:5], "-print-libgcc-file-name"], text=True).strip()
        run(str(linker), "-flavor", "gnu", "-m", "armelf", "-pie", "--no-dynamic-linker",
            "--gc-sections", "--no-undefined", "-T", str(ROOT / "memory.x"),
            "-Map=" + str(OUT / "doom.map"), "-o", str(OUT / "doom.bin"), *objects,
            "--start-group", str(PICOLIBC / "lib/libc.a"), libgcc, "--end-group")
    run("arm-none-eabi-strip", "--strip-debug", str(OUT / "doom.bin"))
    check_elf((OUT / "doom.bin").read_bytes())
    run("arm-none-eabi-size", str(OUT / "doom.bin"))
    print(OUT / "doom.bin")


if __name__ == "__main__":
    main()

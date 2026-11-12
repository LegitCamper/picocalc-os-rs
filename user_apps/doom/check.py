#!/usr/bin/env python3
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent.parent
SLOT_COUNT = 18
SLOT_BYTES = SLOT_COUNT * 4


def check_elf(data):
    header = struct.unpack_from("<16sHHIIIIIHHHHHH", data)
    ident, kind, machine, version, entry, phoff, shoff, flags, _, phsize, phcount, shsize, shcount, _ = header
    assert ident[:7] == b"\x7fELF\x01\x01\x01" and kind == 3 and machine == 40 and version == 1
    assert flags & 0x400 and entry & 1, "requires hard-float ABI and Thumb entry"
    assert phsize == 32 and shsize == 40
    assert phoff + phcount * phsize <= len(data) and shoff + shcount * shsize <= len(data)
    programs = [struct.unpack_from("<8I", data, phoff + index * phsize) for index in range(phcount)]
    assert all(program[0] not in (3, 7) for program in programs), "interpreter/TLS unsupported"
    loads = [program for program in programs if program[0] == 1]
    assert loads and min(program[2] for program in loads) == 0
    extent = max(program[2] + program[5] for program in loads)
    source = (REPO / "kernel/src/elf_format.rs").read_text()
    ceiling = int(re.search(r'#\[cfg\(feature = "psram"\)\]\npub const MAX_IMAGE_SIZE: usize = (\d+) \* 1024;',
                            source)[1]) * 1024
    assert extent <= ceiling, "loaded image exceeds the kernel's PSRAM image ceiling"
    assert extent + 3 * 1024 * 1024 + 320 * 320 * 2 <= 4 * 1024 * 1024
    image = bytearray(extent)
    for _, offset, address, _, file_size, memory_size, _, _ in loads:
        assert file_size <= memory_size and offset + file_size <= len(data)
        image[address:address + file_size] = data[offset:offset + file_size]
    assert any(program[6] & 1 and program[2] <= (entry & ~1) < program[2] + program[5]
               for program in loads), "entry outside executable segment"
    sections = [struct.unpack_from("<10I", data, shoff + index * shsize) for index in range(shcount)]
    relocated = []
    table = None
    for _, section_type, _, _, offset, size, link, _, _, stride in sections:
        assert section_type != 4, "RELA unsupported by kernel"
        if section_type != 8:
            assert offset + size <= len(data)
        if section_type == 9:
            assert stride == 8 and size % stride == 0
            for position in range(offset, offset + size, stride):
                address, info = struct.unpack_from("<II", data, position)
                assert info == 23, "kernel supports only R_ARM_RELATIVE without symbols"
                assert address % 4 == 0 and address + 4 <= extent
                assert address not in relocated, "duplicate relocation"
                value, = struct.unpack_from("<I", image, address)
                assert value < extent, "relative pointer outside image"
                struct.pack_into("<I", image, address, value + 0x11040000)
                relocated.append(address)
        if section_type == 2:
            assert stride == 16 and size % stride == 0
            strings = sections[link]
            names = data[strings[4]:strings[4] + strings[5]]
            for position in range(offset, offset + size, stride):
                name, value, symbol_size, _, _, index = struct.unpack_from("<IIIBBH", data, position)
                label = names[name:].split(b"\0", 1)[0]
                assert not label or index != 0, f"unresolved symbol: {label!r}"
                if label == b"SYS_CALL_TABLE":
                    assert table is None and symbol_size == SLOT_COUNT * 4
                    table = value
        if section_type == 6:
            assert stride == 8
            for position in range(offset, offset + size, stride):
                tag, _ = struct.unpack_from("<II", data, position)
                assert tag != 1, "dynamic libraries unsupported"
    assert table is not None and table % 4 == 0 and table + SLOT_BYTES <= extent
    assert image[table:table + SLOT_BYTES] == bytes(SLOT_BYTES), "syscall table must start zeroed"
    struct.pack_into(f"<{SLOT_COUNT}I", image, table,
                     *(0x10000101 + index * 4 for index in range(SLOT_COUNT)))
    assert struct.unpack_from("<I", image, table + SLOT_BYTES - 4)[0] == 0x10000101 + (SLOT_COUNT - 1) * 4
    assert relocated
    print(f"ELF: {extent} loaded bytes; {len(relocated)} relative relocations; "
          f"{SLOT_COUNT}-slot syscall table; no imports")
    return extent


def main():
    abi = (REPO / "userlib_sys/src/lib.rs").read_text()
    table = re.search(r"pub enum SyscallTable \{(.*?)\n\}", abi, re.S)[1]
    slots = dict((name, int(value)) for name, value in re.findall(r"(\w+) = (\d+)", table))
    assert len(slots) == SLOT_COUNT and {name: slots[name] for name in (
        "Alloc", "Dealloc", "PrintString", "SleepMs", "GetMs", "GetKey", "ReadFile", "FileLen",
        "FillRect", "Blit", "GetLastError"
    )} == {"Alloc": 0, "Dealloc": 1, "PrintString": 2, "SleepMs": 3, "GetMs": 4,
           "GetKey": 6, "ReadFile": 9, "FileLen": 11, "FillRect": 15, "Blit": 16,
           "GetLastError": 17}
    with tempfile.TemporaryDirectory(prefix="picocalc-doom-test-") as temporary:
        staging = Path(temporary)
        source = staging / "doomgeneric"
        source.mkdir()
        shutil.copyfile(ROOT / "doomgeneric/doomgeneric/i_system.c", source / "i_system.c")
        for patch in ("shrink-tunables.patch", "picocalc.patch"):
            subprocess.run(["git", "apply", "--include=doomgeneric/i_system.c", str(ROOT / patch)],
                           cwd=staging, check=True)
        compiler = os.environ.get("CC", "cc")
        engine = str(staging / "i_system.o")
        subprocess.run([compiler, "-std=gnu11", "-fsanitize=address,undefined",
                        "-fno-omit-frame-pointer", "-ffunction-sections", "-fdata-sections",
                        "-DPICOCALC", "-Dexit=port_exit", "-I" + str(ROOT / "doomgeneric/doomgeneric"),
                        "-c", str(source / "i_system.c"), "-o", engine], check=True)
        executable = str(Path(temporary) / "test_platform")
        subprocess.run([compiler, "-std=gnu11", "-Wall", "-Wextra", "-Werror",
                        "-fsanitize=address,undefined", "-fno-omit-frame-pointer", "-DCMAP256",
                        "-DDOOMGENERIC_RESX=320", "-DDOOMGENERIC_RESY=200",
                        "-I" + str(ROOT / "doomgeneric/doomgeneric"), str(ROOT / "test_platform.c"),
                        engine, "-Wl,--gc-sections", "-o", executable], check=True)
        subprocess.run([executable], check=True)
    check_elf((REPO / "target/doom/doom.bin").read_bytes())


if __name__ == "__main__":
    main()

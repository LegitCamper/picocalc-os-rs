#!/usr/bin/env python3
"""Run after a pimoroni2w build: python3 tools/check-psram-ram.py [ELF]."""

import re
import subprocess
import sys

elf = sys.argv[1] if len(sys.argv) > 1 else "target/thumbv8m.main-none-eabihf/release/kernel"
symbols = subprocess.check_output(["arm-none-eabi-objdump", "-t", elf], text=True)
matches = [line.split() for line in symbols.splitlines() if "enter_psram_quad_mode" in line]
assert len(matches) == 1, "Expected one RAM-only PSRAM command function"
symbol = matches[0]
start = int(symbol[0], 16)
end = start + int(symbol[-2], 16)
assert symbol[-3] == ".data" and 0x20000000 <= start < end <= 0x20082000
assembly = subprocess.check_output(
    ["arm-none-eabi-objdump", "-d", f"--disassemble={symbol[-1]}", elf], text=True
)
instructions = re.findall(
    r"^\s*([0-9a-f]+):\s+(?:[0-9a-f]{4}\s+)+\s*([a-z][a-z0-9.]*)\s*(.*)$",
    assembly,
    re.MULTILINE,
)
assert instructions, "No PSRAM instructions decoded"
for address, mnemonic, operands in instructions:
    assert mnemonic not in ("bl", "blx"), f"Call with XIP disabled: {address}: {operands}"
    if mnemonic == "bx":
        assert operands.strip() == "lr", f"Indirect branch: {address}: {operands}"
    if mnemonic.startswith("b") and mnemonic != "bx":
        target = re.match(r"([0-9a-f]+)\s+<", operands)
        assert target and start <= int(target[1], 16) < end, operands
print("PSRAM command function stays in SRAM; no calls or external branches")

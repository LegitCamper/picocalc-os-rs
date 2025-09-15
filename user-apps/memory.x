MEMORY
{
  /* Must match the USERAPP region in the kernel linker script */
  RAM : ORIGIN = 0x20010000, LENGTH = 192K
}

SECTIONS
{
  /* Reserve first 1KB for patchable symbols */
  .user_reloc (NOLOAD) : ALIGN(4)
  {
      __user_reloc_start = .;
      KEEP(*(.user_reloc*));
      __user_reloc_end = .;
  } > RAM

  .text : ALIGN(4)
  {
      *(.text .text.*);
      *(.rodata .rodata.*);
  } > RAM

  .data : ALIGN(4)
  {
      *(.data .data.*);
  } > RAM

  .bss : ALIGN(4)
  {
      *(.bss .bss.*);
      *(COMMON);
  } > RAM
}

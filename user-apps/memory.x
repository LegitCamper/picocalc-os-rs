MEMORY
{
  RAM : ORIGIN = 0x0, LENGTH = 250K
}

SECTIONS
{
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

  .syscall_table (NOLOAD) : ALIGN(4)
  {
      __user_reloc_start = .;
      KEEP(*(.user_reloc*));
      __user_reloc_end = .;
  } > RAM
}

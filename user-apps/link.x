MEMORY
{
  RAM : ORIGIN = 0x00000000, LENGTH = 256K
}

SECTIONS
{
  .text : {
    *(.text .text.*);
    *(.rodata .rodata.*);
  } > RAM

  .data : {
    *(.data .data.*);
  } > RAM

  .bss : {
    *(.bss .bss.*);
    *(COMMON);
  } > RAM
}

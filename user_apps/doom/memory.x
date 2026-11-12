ENTRY(_start)

MEMORY { RAM : ORIGIN = 0, LENGTH = 640K }

SECTIONS
{
    .text : ALIGN(8) { *(.text .text.*) *(.rodata .rodata.*) } > RAM
    .data : ALIGN(8) { *(.data .data.*) *(.got .got.*) } > RAM
    .bss (NOLOAD) : ALIGN(8) { *(.bss .bss.*) *(COMMON) } > RAM
    .syscall_table (NOLOAD) : ALIGN(8) { KEEP(*(.syscall_table)) } > RAM
}

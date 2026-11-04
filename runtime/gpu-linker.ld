ENTRY(_start)

PHDRS
{
  text PT_LOAD FLAGS(5);
  rodata PT_LOAD FLAGS(4);
  data PT_LOAD FLAGS(6);
  stack PT_GNU_STACK FLAGS(6);
  tls PT_TLS FLAGS(4);
}

SECTIONS
{
  . = 0x4E000000;

  .text : ALIGN(4K) {
    /* If the TLS provider object is extracted, retain its real initializer.
       Exception CIEs may keep compiler TLS even when the Rust caller of its
       initializer is discarded. Runtime::initialize separately verifies the
       current task's TCB before any C++ constructor is called. */
    KEEP(*(.text.osaiku_llama_tls_prepare))
    *(.text .text.*)
  } :text

  .rodata : ALIGN(4K) {
    *(.rodata .rodata.*)
    /* Static GOT must precede PT_TLS; orphan placement truncates .tbss. */
    . = ALIGN(8);
    *(.got .got.*)
    . = ALIGN(8);
    __osaiku_init_array_start = .;
    KEEP(*(SORT_BY_INIT_PRIORITY(.init_array.*)))
    KEEP(*(.init_array))
    __osaiku_init_array_end = .;
    __osaiku_fini_array_start = .;
    KEEP(*(SORT_BY_INIT_PRIORITY(.fini_array.*)))
    KEEP(*(.fini_array))
    __osaiku_fini_array_end = .;
  } :rodata

  .eh_frame : ALIGN(8) {
    __eh_frame_start = .;
    /* The linker retains unwind records for live code. KEEP would also root
       personalities from discarded C++ functions, pulling exception TLS into
       a Rust-only executable without its owning compute-task startup. */
    *(.eh_frame .eh_frame.*)
    __eh_frame_end = .;
  } :rodata
  .eh_frame_hdr : ALIGN(8) { *(.eh_frame_hdr) } :rodata
  __eh_frame_hdr_start = SIZEOF(.eh_frame_hdr) > 0 ? ADDR(.eh_frame_hdr) : 0;
  __eh_frame_hdr_end = SIZEOF(.eh_frame_hdr) > 0 ? ADDR(.eh_frame_hdr) + SIZEOF(.eh_frame_hdr) : 0;
  .gcc_except_table : ALIGN(8) { *(.gcc_except_table .gcc_except_table.*) } :rodata

  .tdata : ALIGN(4K) {
    __osaiku_tls_start = .;
    *(.tdata .tdata.*)
    __osaiku_tls_file_end = .;
  } :rodata :tls
  .tbss : { *(.tbss .tbss.*) } :rodata :tls
  __osaiku_tls_alignment = MAX(ALIGNOF(.tdata), ALIGNOF(.tbss));
  __osaiku_tls_span = ALIGN(ADDR(.tbss) + SIZEOF(.tbss) - ADDR(.tdata), __osaiku_tls_alignment);
  ASSERT(__osaiku_tls_alignment <= 4096 && __osaiku_tls_span <= 16384,
         "graphics compiler TLS exceeds admitted per-task storage")
  ASSERT((SIZEOF(.tdata) == 0 && SIZEOF(.tbss) == 0) || DEFINED(osaiku_llama_tls_prepare),
         "graphics compiler TLS requires a qualified per-task runtime")

  /* TLS NOBITS need not advance the location counter. In the .tbss-only
     case, keep the read-only template's LOAD pages disjoint from RW data. */
  . = ALIGN(MAX(., ADDR(.tbss) + SIZEOF(.tbss)), 4K);
  .data : ALIGN(4K) {
    *(.data .data.*)
  } :data

  .bss : ALIGN(4K) {
    *(.bss .bss.*)
    *(COMMON)
  } :data

}

# RUN: %as %s -o %t.o
# RUN: %uld -o %t.exe %t.o
# RUN: %t.exe || echo "Exit: $?" | %filecheck %s

# CHECK: Exit: 42

# Test global data access (absolute relocation)
.global _start
_start:
    mov exit_code(%rip), %rdi   # load from global data
    mov $60, %rax               # syscall number for exit
    syscall

.section .rodata
exit_code:
    .long 42

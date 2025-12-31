# RUN: %as %s -o %t.o
# RUN: %uld -o %t.exe %t.o
# RUN: %t.exe || echo "Exit: $?" | %filecheck %s

# CHECK: Exit: 42

# Test BSS section (uninitialized data)
.global _start
_start:
    # Store value in BSS
    mov $42, %rax
    mov %rax, buffer(%rip)

    # Read it back and exit
    mov buffer(%rip), %rdi
    mov $60, %rax
    syscall

.section .bss
buffer:
    .skip 8

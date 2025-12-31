# RUN: %as %s -o %t.o
# RUN: %uld -o %t.exe %t.o
# RUN: %t.exe || echo "Exit: $?" | %filecheck %s

# CHECK: Exit: 84

# Test writable data (modify global and return)
.global _start
_start:
    # Load, double, and store
    mov counter(%rip), %rax
    add %rax, %rax              # double it: 42 * 2 = 84
    mov %rax, counter(%rip)

    # Exit with the value
    mov counter(%rip), %rdi
    mov $60, %rax
    syscall

.section .data
counter:
    .quad 42

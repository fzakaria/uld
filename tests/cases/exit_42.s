# RUN: %as %s -o %t.o
# RUN: %uld -o %t.exe %t.o
# RUN: %t.exe || echo "Exit: $?" | %filecheck %s

# CHECK: Exit: 42

# Simplest test: just call exit(42) syscall
.global _start
_start:
    mov $60, %rax   # syscall number for exit
    mov $42, %rdi   # exit code
    syscall

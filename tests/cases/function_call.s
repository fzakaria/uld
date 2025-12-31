# RUN: %as %s -o %t.o
# RUN: %uld -o %t.exe %t.o
# RUN: %t.exe || echo "Exit: $?" | %filecheck %s

# CHECK: Exit: 42

# Test function call (PC-relative relocation)
.global _start
_start:
    call get_exit_code
    mov %rax, %rdi      # exit code from return value
    mov $60, %rax       # syscall number for exit
    syscall

get_exit_code:
    mov $42, %rax
    ret

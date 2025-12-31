# Minimal startup code for C programs without libc
# Calls main() and exits with its return value

.global _start
.extern main

_start:
    # Call main()
    call main

    # Exit with return value from main (in %eax)
    mov %eax, %edi
    mov $60, %eax       # exit syscall
    syscall

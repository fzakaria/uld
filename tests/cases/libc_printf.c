// RUN: %clang -c %s -o %t.o
// RUN: %uld -o %t.exe %crt1 %crti %t.o %libc %crtn
// RUN: (%t.exe; echo "Exit: $?") | %filecheck %s

// CHECK: Hello from uld
// CHECK: Exit: 42

// Test linking with musl libc (printf, static linking with CRT files)
#include <stdio.h>

int main(void) {
    printf("Hello from uld\n");
    return 42;
}

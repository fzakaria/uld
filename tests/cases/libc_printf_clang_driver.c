// RUN: %clang -c %s -o %t.o
// RUN: %clang -fuse-ld=%uld -static -o %t.exe %t.o
// RUN: (%t.exe; echo "Exit: $?") | %filecheck %s

// CHECK: Hello from uld
// CHECK: Exit: 42

// Test linking with musl libc using clang as the driver
#include <stdio.h>

int main(void) {
    printf("Hello from uld\n");
    return 42;
}

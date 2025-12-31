// RUN: %clang -c %s -o %t.o
// RUN: %clang -fuse-ld=%uld -static -o %t.exe %t.o
// RUN: %t.exe || echo "Exit: $?" | %filecheck %s

// CHECK: Hello from musl!
// CHECK: Exit: 0

#include <stdio.h>

int main() {
    printf("Hello from musl!\n");
    return 0;
}

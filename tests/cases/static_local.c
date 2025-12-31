// RUN: %clang -c %s -o %t.o
// RUN: %clang -fuse-ld=%uld -static -o %t.exe %t.o
// RUN: (%t.exe; echo "Exit: $?") | %filecheck %s

// CHECK: static_var=42
// CHECK: after increment=43
// CHECK: Exit: 0

// Test static local variables
#include <stdio.h>

void counter(void) {
    static int static_var = 42;
    printf("static_var=%d\n", static_var);
    static_var++;
    printf("after increment=%d\n", static_var);
}

int main(void) {
    counter();
    return 0;
}

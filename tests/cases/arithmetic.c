// RUN: %cc -c %s -o %t.o
// RUN: %cc -fuse-ld=%uld -static -o %t.exe %t.o
// RUN: (%t.exe; echo "Exit: $?") | %filecheck %s

// CHECK: add=15
// CHECK: sub=5
// CHECK: mul=50
// CHECK: div=2
// CHECK: mod=0
// CHECK: Exit: 0

// Test arithmetic operations
#include <stdio.h>

int main(void) {
    int a = 10, b = 5;
    printf("add=%d\n", a + b);
    printf("sub=%d\n", a - b);
    printf("mul=%d\n", a * b);
    printf("div=%d\n", a / b);
    printf("mod=%d\n", a % b);
    return 0;
}

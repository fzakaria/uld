// RUN: %cc -c %s -o %t.o
// RUN: %cc -fuse-ld=%uld -static -o %t.exe %t.o
// RUN: (%t.exe; echo "Exit: $?") | %filecheck %s

// CHECK: fib(10) = 55
// CHECK: Exit: 0

// Test recursive function calls
#include <stdio.h>

int fib(int n) {
    if (n <= 1) return n;
    return fib(n - 1) + fib(n - 2);
}

int main(void) {
    printf("fib(10) = %d\n", fib(10));
    return 0;
}

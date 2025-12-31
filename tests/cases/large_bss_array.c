// RUN: %clang -c %s -o %t.o
// RUN: %clang -fuse-ld=%uld -static -o %t.exe %t.o
// RUN: (%t.exe; echo "Exit: $?") | %filecheck %s

// CHECK: Array sum: 4950
// CHECK: Exit: 0

// Test large BSS array and loops
#include <stdio.h>

int data[100];  // BSS

int main(void) {
    int sum = 0;
    for (int i = 0; i < 100; i++) {
        data[i] = i;
    }
    for (int i = 0; i < 100; i++) {
        sum += data[i];
    }
    printf("Array sum: %d\n", sum);
    return 0;
}

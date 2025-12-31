// RUN: %clang -c %s -o %t.o
// RUN: %clang -fuse-ld=%uld -static -o %t.exe %t.o
// RUN: (%t.exe; echo "Exit: $?") | %filecheck %s

// CHECK: ptr = 0x
// CHECK: Exit: 0

// Test pointer arithmetic and formatting
#include <stdio.h>

int arr[5] = {1, 2, 3, 4, 5};

int main(void) {
    int *ptr = &arr[2];
    printf("ptr = %p\n", (void*)ptr);
    return 0;
}

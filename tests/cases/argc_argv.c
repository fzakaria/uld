// RUN: %clang -c %s -o %t.o
// RUN: %clang -fuse-ld=%uld -static -o %t.exe %t.o
// RUN: (%t.exe; echo "Exit: $?") | %filecheck %s

// CHECK: argc=1
// CHECK: argv[0]=
// CHECK: Exit: 0

// Test argc/argv handling
#include <stdio.h>

int main(int argc, char *argv[]) {
    printf("argc=%d\n", argc);
    printf("argv[0]=%s\n", argv[0]);
    return 0;
}

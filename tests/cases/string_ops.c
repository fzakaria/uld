// RUN: %cc -c %s -o %t.o
// RUN: %cc -fuse-ld=%uld -static -o %t.exe %t.o
// RUN: (%t.exe; echo "Exit: $?") | %filecheck %s

// CHECK: Length: 13
// CHECK: First: H, Last: !
// CHECK: Exit: 0

// Test string operations (rodata, string functions)
#include <stdio.h>
#include <string.h>

const char *message = "Hello, World!";

int main(void) {
    size_t len = strlen(message);
    printf("Length: %zu\n", len);
    printf("First: %c, Last: %c\n", message[0], message[len-1]);
    return 0;
}

// RUN: %clang -c %S/../start.s -o %t.start.o
// RUN: %clang -c %s -o %t.o
// RUN: %clang -fuse-ld=%uld -nostdlib -o %t.exe %t.start.o %t.o
// RUN: %t.exe || echo "Exit: $?" | %filecheck %s

// CHECK: Exit: 123

int main() {
    return 123;
}

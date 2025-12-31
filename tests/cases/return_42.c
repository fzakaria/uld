// RUN: %clang -c %s -o %t.o
// RUN: %clang -fuse-ld=%uld -static -o %t.exe %t.o
// RUN: %t.exe || echo "Exit: $?" | %filecheck %s

// CHECK: Exit: 42

int main() {
    return 42;
}
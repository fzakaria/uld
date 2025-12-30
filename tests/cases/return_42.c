// RUN: %clang -c %S/../start.s -o %t.start.o
// RUN: %clang -c %s -o %t.o
// RUN: %cargo_run -o %t.exe %t.start.o %t.o
// RUN: %t.exe || echo "Exit: $?" | %filecheck %s

// CHECK: Exit: 42

int main() {
    return 42;
}

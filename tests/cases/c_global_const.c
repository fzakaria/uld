// RUN: %cc -c %s -o %t.o -ffreestanding
// RUN: %as %start -o %t_start.o
// RUN: %uld -o %t.exe %t_start.o %t.o
// RUN: %t.exe || echo "Exit: $?" | %filecheck %s

// CHECK: Exit: 42

// Test global constant data access
static const int value = 42;

int main(void) {
    return value;
}

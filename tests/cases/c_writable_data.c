// RUN: %cc -c %s -o %t.o -ffreestanding
// RUN: %as %start -o %t_start.o
// RUN: %uld -o %t.exe %t_start.o %t.o
// RUN: %t.exe || echo "Exit: $?" | %filecheck %s

// CHECK: Exit: 84

// Test writable global data
static int counter = 42;

int main(void) {
    counter = counter * 2;  // 42 * 2 = 84
    return counter;
}

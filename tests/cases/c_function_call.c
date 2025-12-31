// RUN: %cc -c %s -o %t.o -ffreestanding
// RUN: %as %start -o %t_start.o
// RUN: %uld -o %t.exe %t_start.o %t.o
// RUN: %t.exe || echo "Exit: $?" | %filecheck %s

// CHECK: Exit: 42

// Test function calls (PC-relative relocations)
static int get_value(void) {
    return 42;
}

int main(void) {
    return get_value();
}

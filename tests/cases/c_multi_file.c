// RUN: %cc -c %s -o %t.o -ffreestanding
// RUN: %cc -c %helper -o %t_helper.o -ffreestanding
// RUN: %as %start -o %t_start.o
// RUN: %uld -o %t.exe %t_start.o %t.o %t_helper.o
// RUN: %t.exe || echo "Exit: $?" | %filecheck %s

// CHECK: Exit: 42

// Test cross-file function calls (external symbol resolution)
extern int get_value(void);

int main(void) {
    return get_value();
}

#include <stdio.h>
#include <stdlib.h>

#define MAX_SIZE 100
#define SQUARE(x) ((x) * (x))

int global_var = 42;

int add(int a, int b) {
    return a + b;
}

int subtract(int a, int b) {
    return a - b;
}

struct Point {
    int x;
    int y;
};

enum Color {
    RED,
    GREEN,
    BLUE
};

typedef struct {
    int width;
    int height;
} Rectangle;

int main() {
    int result1 = add(5, 3);
    int result2 = subtract(10, 4);
    printf("Results: %d, %d\n", result1, result2);
    
    struct Point p = {1, 2};
    Rectangle r = {10, 20};
    
    global_var = 100;
    
    int squared = SQUARE(5);
    
    return 0;
}
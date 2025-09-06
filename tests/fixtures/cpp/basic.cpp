#include <iostream>
#include <vector>
#include <string>

#define MAX_ITEMS 50

int global_counter = 0;

class Base {
public:
    virtual void method() {
        std::cout << "Base method" << std::endl;
    }
    
    virtual void another_method() = 0;
};

class Derived : public Base {
public:
    void method() override {
        std::cout << "Derived method" << std::endl;
    }
    
    void another_method() override {
        std::cout << "Derived another method" << std::endl;
    }
    
    void specific_method() {
        method();
    }
};

class AnotherClass {
public:
    void do_something() {
        Derived d;
        d.method();
        d.another_method();
    }
};

namespace Utils {
    int utility_function(int x) {
        return x * 2;
    }
    
    class Helper {
    public:
        static void help() {
            std::cout << "Helping" << std::endl;
        }
    };
}

int add(int a, int b) {
    return a + b;
}

int calculate() {
    int local_var = 10;
    int result = add(local_var, 5);
    global_counter += result;
    return result;
}

int main() {
    Derived obj;
    obj.method();
    obj.another_method();
    obj.specific_method();
    
    AnotherClass ac;
    ac.do_something();
    
    int sum = add(3, 7);
    int calc_result = calculate();
    
    Utils::utility_function(5);
    Utils::Helper::help();
    
    std::vector<int> numbers = {1, 2, 3, 4, 5};
    for (int n : numbers) {
        std::cout << n << " ";
    }
    std::cout << std::endl;
    
    return 0;
}
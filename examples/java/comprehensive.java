package com.example.demo;

import java.util.List;
import java.util.ArrayList;
import java.util.Map;
import static java.lang.Math.PI;

/**
 * Comprehensive Java test file
 * Contains all major language features for parser testing
 */
public class ComprehensiveExample<T extends Comparable<T>> {

    // Fields with different visibility modifiers
    private String privateField;
    protected int protectedField;
    public List<String> publicField;
    String packagePrivateField;  // no modifier = package-private

    private static final double CONSTANT = 3.14159;
    private static int staticField = 42;

    /**
     * Default constructor
     */
    public ComprehensiveExample() {
        this.privateField = "default";
        this.protectedField = 0;
        this.publicField = new ArrayList<>();
    }

    /**
     * Parameterized constructor
     * @param value Initial value
     */
    public ComprehensiveExample(String value) {
        this.privateField = value;
        this.protectedField = value.length();
        this.publicField = new ArrayList<>();
    }

    // Public method
    public String getPrivateField() {
        return privateField;
    }

    // Private method
    private void privateMethod() {
        System.out.println("Private method");
    }

    // Protected method
    protected void protectedMethod() {
        System.out.println("Protected method");
    }

    // Package-private method
    void packagePrivateMethod() {
        System.out.println("Package private method");
    }

    // Static method
    public static int staticMethod() {
        return staticField;
    }

    // Method with generics
    public <K, V> Map<K, V> genericMethod(K key, V value) {
        return Map.of(key, value);
    }

    // Method with varargs
    public void varArgsMethod(String... args) {
        for (String arg : args) {
            System.out.println(arg);
        }
    }

    // Method with throws clause
    public void throwsMethod() throws Exception {
        throw new Exception("Test exception");
    }

    // Final method
    public final void finalMethod() {
        System.out.println("Cannot be overridden");
    }

    /**
     * Inner class
     */
    public class InnerClass {
        private String innerField;

        public InnerClass(String value) {
            this.innerField = value;
        }

        public String getInnerField() {
            return innerField;
        }
    }

    /**
     * Static nested class
     */
    public static class StaticNestedClass {
        private int nestedValue;

        public StaticNestedClass(int value) {
            this.nestedValue = value;
        }

        public int getNestedValue() {
            return nestedValue;
        }
    }

    // Anonymous class example
    public Runnable createRunnable() {
        return new Runnable() {
            @Override
            public void run() {
                System.out.println("Anonymous class");
            }
        };
    }
}

/**
 * Interface example
 */
interface ExampleInterface {
    void interfaceMethod();

    default void defaultMethod() {
        System.out.println("Default method");
    }

    static void staticInterfaceMethod() {
        System.out.println("Static interface method");
    }
}

/**
 * Abstract class example
 */
abstract class AbstractExample {
    abstract void abstractMethod();

    void concreteMethod() {
        System.out.println("Concrete method");
    }
}

/**
 * Class with implements
 */
class ImplementationExample implements ExampleInterface {
    @Override
    public void interfaceMethod() {
        System.out.println("Implemented method");
    }
}

/**
 * Class with extends
 */
class ExtensionExample extends AbstractExample {
    @Override
    void abstractMethod() {
        System.out.println("Implemented abstract method");
    }
}

/**
 * Enum example
 */
enum Color {
    RED("Red", 0xFF0000),
    GREEN("Green", 0x00FF00),
    BLUE("Blue", 0x0000FF);

    private final String name;
    private final int hexValue;

    Color(String name, int hexValue) {
        this.name = name;
        this.hexValue = hexValue;
    }

    public String getName() {
        return name;
    }

    public int getHexValue() {
        return hexValue;
    }
}

/**
 * Annotation example
 */
@interface CustomAnnotation {
    String value();
    int count() default 1;
}

/**
 * Generic interface example
 */
interface GenericInterface<T> {
    T getValue();
    void setValue(T value);
}

/**
 * Multiple type parameters
 */
class MultipleGenerics<K extends Comparable<K>, V> implements GenericInterface<V> {
    private K key;
    private V value;

    public MultipleGenerics(K key, V value) {
        this.key = key;
        this.value = value;
    }

    public K getKey() {
        return key;
    }

    @Override
    public V getValue() {
        return value;
    }

    @Override
    public void setValue(V value) {
        this.value = value;
    }
}

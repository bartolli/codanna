<?php

class Base {
    public function hello() {}
}

class Child extends Base {
    public function go() {
        parent::hello();
    }
}

class Counter {
    public static function reset() {}

    public function bump() {
        self::reset();
    }

    public function rebump() {
        static::reset();
    }
}

<?php

/** Adds two numbers. */
function add($a, $b) {
    return $a + $b;
}

class CalcTest {
    public function testAdd() {
        add(1, 2);
    }
}

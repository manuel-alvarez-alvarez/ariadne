class Calculator {
    /** Adds two numbers. */
    fun add(a: Int, b: Int): Int {
        return a + b
    }
}

class CalculatorTest {
    @Test
    fun addsNumbers() {
        assertEquals(3, Calculator().add(1, 2))
    }
}

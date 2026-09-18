public class Calculator {
    /** Adds two numbers. */
    public int add(int a, int b) {
        return a + b;
    }

    @Test
    public void addsNumbers() {
        assertEquals(3, add(1, 2));
    }
}

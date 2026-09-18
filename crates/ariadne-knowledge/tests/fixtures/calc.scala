object Calculator {
  /** Adds two numbers. */
  def add(a: Int, b: Int): Int = {
    a + b
  }
}

class CalculatorSuite {
  test("adds") {
    Calculator.add(1, 2)
  }
}

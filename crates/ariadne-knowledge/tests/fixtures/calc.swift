class Calculator {
    /// Adds two numbers.
    func add(_ a: Int, _ b: Int) -> Int {
        return a + b
    }
}

class CalculatorTests: XCTestCase {
    func testAdds() {
        XCTAssertEqual(Calculator().add(1, 2), 3)
    }
}

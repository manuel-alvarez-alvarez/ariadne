/// Adds two numbers.
int add(int a, int b) {
  return a + b;
}

void main() {
  test('adds', () {
    expect(add(1, 2), 3);
  });
}

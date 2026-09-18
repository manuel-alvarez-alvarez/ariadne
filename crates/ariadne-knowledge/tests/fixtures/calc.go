package calc

// Add adds two numbers.
func Add(a, b int) int {
	return a + b
}

func TestAdd(t *testing.T) {
	if Add(1, 2) != 3 {
		t.Fail()
	}
}

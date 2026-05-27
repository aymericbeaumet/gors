package math

func multiply(a int, b int) int {
	return a * b
}

func Square(x int) int {
	return multiply(x, x)
}

func Cube(x int) int {
	return multiply(multiply(x, x), x)
}

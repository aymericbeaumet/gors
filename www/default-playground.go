package main

func triangular(limit int) int {
	total := 0
	for value := 1; value <= limit; value++ {
		total += value
	}
	return total
}

func parity(value int) string {
	if value%2 == 0 {
		return "even"
	}
	return "odd"
}

func square(value int) int {
	return value * value
}

func main() {
	for value := 1; value <= 5; value++ {
		result := triangular(value)
		println(value, parity(result), result, square(result))
	}
}

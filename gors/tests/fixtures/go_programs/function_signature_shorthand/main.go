package main

import "fmt"

func add(a, b int) int {
	return a + b
}

func format(label string, a, b int) {
	fmt.Println(label, add(a, b))
}

func main() {
	format("sum", 20, 22)
}

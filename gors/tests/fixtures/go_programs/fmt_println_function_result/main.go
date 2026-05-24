package main

import "fmt"

func double(n int) int {
	return n * 2
}

func add(a int, b int) int {
	return a + b
}

func main() {
	fmt.Println(double(21))
	fmt.Println(add(3, 4))
	fmt.Println(double(add(5, 10)))
}

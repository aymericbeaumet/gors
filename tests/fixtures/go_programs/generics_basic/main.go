package main

import "fmt"

func Max[T int | float64](a T, b T) T {
	if a > b {
		return a
	}
	return b
}

func main() {
	fmt.Println(Max(3, 5))
	fmt.Println(Max(2.5, 1.5))
}

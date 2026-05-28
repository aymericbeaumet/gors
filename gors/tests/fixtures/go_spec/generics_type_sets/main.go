package main

import "fmt"

type OrderedNumber interface {
	~int | ~float64
}

func Equal[T comparable](left T, right T) bool {
	return left == right
}

func Larger[T OrderedNumber](left T, right T) T {
	if left > right {
		return left
	}
	return right
}

func main() {
	fmt.Println(Equal("go", "go"), Equal(3, 4), Larger(4, 7), Larger(2.5, 1.5))
}

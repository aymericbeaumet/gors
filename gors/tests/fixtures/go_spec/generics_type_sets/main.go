package main

import "fmt"

type SliceOf[E any] interface {
	~[]E
}

type OrderedNumber interface {
	~int | ~float64
}

func Index[S SliceOf[E], E comparable](values S, target E) int {
	for index, value := range values {
		if value == target {
			return index
		}
	}
	return -1
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
	values := []int{3, 5, 8}
	fmt.Println(Index(values, 5), Index(values, 13), Equal("go", "go"), Equal(3, 4), Larger(4, 7), Larger(2.5, 1.5))
}

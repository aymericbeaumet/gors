package main

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
	if Index(values, 5) != 1 {
		panic("generic slice constraint index mismatch")
	}
	if Index(values, 13) != -1 {
		panic("generic slice constraint missing value mismatch")
	}
	if !Equal("go", "go") {
		panic("generic comparable string equality mismatch")
	}
	if Equal(3, 4) {
		panic("generic comparable int equality mismatch")
	}
	if Larger(4, 7) != 7 {
		panic("generic ordered int comparison mismatch")
	}
	if Larger(2.5, 1.5) != 2.5 {
		panic("generic ordered float comparison mismatch")
	}
}

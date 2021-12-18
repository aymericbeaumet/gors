package main

import "fmt"

func main() {
	const n = uint(10)
	fmt.Println(fibonacci(n))
}

func fibonacci(n uint) uint {
	if m, ok := memoized[n]; ok {
		return m
	}

	out := fibonacci(n - 1)
	out += fibonacci(n - 2)
	memoized[n] = out
	return out
}

var memoized = map[uint]uint{
	0: 0,
	1: 1,
}

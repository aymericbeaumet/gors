package main

import "fmt"

func ints(yield func(int) bool) {
	for i := 0; i < 3; i++ {
		if !yield(i) {
			return
		}
	}
}

func pairs(yield func(string, int) bool) {
	if !yield("a", 1) {
		return
	}
	yield("b", 2)
}

func main() {
	for v := range ints {
		fmt.Println("int", v)
	}
	for k, v := range pairs {
		fmt.Println("pair", k, v)
	}
}

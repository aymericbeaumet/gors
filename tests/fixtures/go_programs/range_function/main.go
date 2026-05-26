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

func firstEven() int {
	for v := range ints {
		if v == 2 {
			return v
		}
	}
	return -1
}

func namedReturn() (out int) {
	out = 10
	for v := range ints {
		if v == 1 {
			return
		}
	}
	return -1
}

func voidReturn() {
	for v := range ints {
		if v == 1 {
			fmt.Println("void return", v)
			return
		}
	}
	fmt.Println("void after")
}

func main() {
	for v := range ints {
		fmt.Println("int", v)
	}
	for k, v := range pairs {
		fmt.Println("pair", k, v)
	}
	for v := range ints {
		if v == 1 {
			continue
		}
		if v == 2 {
			break
		}
		fmt.Println("control", v)
	}
	fmt.Println("first even", firstEven())
	fmt.Println("named return", namedReturn())
	voidReturn()
}

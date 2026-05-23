package main

import "fmt"

func main() {
	// make slice with length
	s := make([]int, 5)
	fmt.Println(len(s))
	fmt.Println(cap(s))

	// make slice with length and capacity
	s2 := make([]int, 3, 10)
	fmt.Println(len(s2))
	fmt.Println(cap(s2))

	// new
	p := new(int)
	fmt.Println(*p)
}

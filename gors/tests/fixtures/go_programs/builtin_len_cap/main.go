package main

import "fmt"

func main() {
	// len on slice
	s := []int{1, 2, 3, 4, 5}
	fmt.Println(len(s))

	// len on string
	str := "hello"
	fmt.Println(len(str))

	// cap on slice
	s2 := make([]int, 3)
	fmt.Println(len(s2))
	fmt.Println(cap(s2))
}

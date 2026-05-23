package main

import "fmt"

func main() {
	// append single element
	s := []int{1, 2, 3}
	s = append(s, 4)
	fmt.Println(len(s))

	// copy
	src := []int{10, 20, 30}
	dst := make([]int, 5)
	n := copy(dst, src)
	fmt.Println(n)
	fmt.Println(len(dst))
}

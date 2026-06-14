package main

import (
	"fmt"
	"slices"
)

func main() {
	values := []int{4, 2, 9, 2}
	fmt.Println(slices.Contains(values, 9), slices.Contains(values, 7))
	fmt.Println(slices.Index(values, 2), slices.Index(values, 7))
}

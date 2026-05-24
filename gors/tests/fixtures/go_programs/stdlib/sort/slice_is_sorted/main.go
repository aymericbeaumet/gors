package main

import (
	"fmt"
	"sort"
)

func keepOrder(i int, j int) bool {
	return i < j
}

func main() {
	values := []int{3, 2, 1}
	fmt.Println(sort.SliceIsSorted(values, keepOrder))
}

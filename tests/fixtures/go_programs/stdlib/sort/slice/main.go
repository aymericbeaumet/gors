package main

import (
	"fmt"
	"sort"
)

func keepOrder(i int, j int) bool {
	return i < j
}

func main() {
	values := []int{1, 2, 3}
	sort.Slice(values, keepOrder)
	fmt.Println(values)
}

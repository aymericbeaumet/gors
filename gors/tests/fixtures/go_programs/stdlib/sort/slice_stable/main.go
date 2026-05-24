package main

import (
	"fmt"
	"sort"
)

func keepOrder(i int, j int) bool {
	return i < j
}

func main() {
	values := []string{"a", "b", "c"}
	sort.SliceStable(values, keepOrder)
	fmt.Println(values)
}

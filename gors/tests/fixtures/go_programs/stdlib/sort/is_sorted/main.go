package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []int{1, 2, 3}
	fmt.Println(sort.IsSorted(sort.IntSlice(values)))
}

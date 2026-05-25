package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []int{1, 2, 3}
	sort.IntSlice(values).Swap(0, 2)
	fmt.Println(values)
}

package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []int{1, 3, 2}
	sort.Sort(sort.Reverse(sort.IntSlice(values)))
	fmt.Println(values)
}

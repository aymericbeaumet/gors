package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []int{3, 1, 2}
	sort.Sort(sort.IntSlice(values))
	fmt.Println(values)
}

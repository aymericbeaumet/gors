package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []int{3, 1, 2}
	sort.IntSlice(values).Sort()
	fmt.Println(values)
}

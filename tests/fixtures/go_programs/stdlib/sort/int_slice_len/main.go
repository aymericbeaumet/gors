package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []int{3, 1, 2}
	fmt.Println(sort.IntSlice(values).Len())
}

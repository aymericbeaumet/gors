package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []int{1, 3, 5, 7}
	fmt.Println(sort.SearchInts(values, 4))
}

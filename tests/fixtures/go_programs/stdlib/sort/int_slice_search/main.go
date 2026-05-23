package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []int{1, 2, 3}
	fmt.Println(sort.IntSlice(values).Search(2))
}

package main

import (
	"fmt"
	"sort"
)

func atLeastSeven(i int) bool {
	return i >= 7
}

func main() {
	fmt.Println(sort.Search(10, atLeastSeven))
}

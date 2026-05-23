package main

import (
	"fmt"
	"sort"
)

func compareToFive(i int) int {
	values := []int{1, 3, 5, 7}
	if 5 < values[i] {
		return -1
	}
	if 5 > values[i] {
		return 1
	}
	return 0
}

func main() {
	idx, found := sort.Find(4, compareToFive)
	fmt.Println(idx, found)
}

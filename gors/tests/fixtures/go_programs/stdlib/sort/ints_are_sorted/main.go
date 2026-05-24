package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []int{1, 2, 3}
	if sort.IntsAreSorted(values) {
		fmt.Println("sorted")
	} else {
		fmt.Println("failed")
	}
}

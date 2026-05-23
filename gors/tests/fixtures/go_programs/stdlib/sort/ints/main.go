package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []int{3, 1, 2}
	sort.Ints(values)
	if values[0] == 1 && values[2] == 3 {
		fmt.Println("sorted")
	} else {
		fmt.Println("failed")
	}
}

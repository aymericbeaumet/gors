package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []float64{-1.25, 0.5, 3.5}
	if sort.Float64sAreSorted(values) {
		fmt.Println("sorted")
	} else {
		fmt.Println("failed")
	}
}

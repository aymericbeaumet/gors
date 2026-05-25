package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []float64{1.25, 3.5, 8.0}
	fmt.Println(sort.SearchFloat64s(values, 3.0))
}

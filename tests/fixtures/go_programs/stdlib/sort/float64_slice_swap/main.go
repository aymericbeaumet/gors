package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []float64{1.25, 2.5, 3.75}
	sort.Float64Slice(values).Swap(0, 2)
	fmt.Println(values)
}

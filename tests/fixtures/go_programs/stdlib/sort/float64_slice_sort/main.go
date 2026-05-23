package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []float64{3.5, -1.25, 0.5}
	sort.Float64Slice(values).Sort()
	fmt.Println(values)
}

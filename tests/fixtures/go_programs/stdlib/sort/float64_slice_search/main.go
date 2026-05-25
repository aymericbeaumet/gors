package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []float64{1.25, 3.5, 8.0}
	fmt.Println(sort.Float64Slice(values).Search(3.0))
}

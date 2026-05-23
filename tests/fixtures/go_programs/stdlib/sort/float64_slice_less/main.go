package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []float64{3.5, 1.25}
	fmt.Println(sort.Float64Slice(values).Less(1, 0))
}

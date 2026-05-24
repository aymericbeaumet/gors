package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []float64{3.5, -1.25, 0.5}
	sort.Float64s(values)
	if values[0] == -1.25 && values[2] == 3.5 {
		fmt.Println("sorted")
	} else {
		fmt.Println("failed")
	}
}

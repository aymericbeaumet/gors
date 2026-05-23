package main

import (
	"fmt"
	"math"
)

func main() {
	fmt.Println(math.IsNaN(math.NaN()))
	fmt.Println(math.IsInf(math.Inf(1), 1))
	fmt.Println(math.Abs(-42.5))
	fmt.Println(math.MaxInt64)
}

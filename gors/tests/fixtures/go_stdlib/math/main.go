package main

import (
	"fmt"
	"math"
)

func main() {
	fmt.Println("== math/basic ==")
	fmt.Println(math.Abs(-3.5))
	fmt.Println(math.Max(3, 7))
	fmt.Println(math.Min(3, 7))
	fmt.Println(math.IsInf(math.Inf(1), 1))
	fmt.Println(math.IsNaN(math.NaN()))
}

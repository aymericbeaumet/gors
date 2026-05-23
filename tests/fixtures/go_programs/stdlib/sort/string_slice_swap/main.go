package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []string{"alpha", "beta", "gamma"}
	sort.StringSlice(values).Swap(0, 2)
	fmt.Println(values)
}

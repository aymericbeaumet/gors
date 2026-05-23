package main

import (
	"fmt"
	"sort"
)

func main() {
	ints := []int{7, 2, 5, 2, 9}
	sort.Ints(ints)
	if sort.IntsAreSorted(ints) && ints[0] == 2 && ints[1] == 2 && ints[4] == 9 {
		fmt.Println("ints sorted")
	} else {
		fmt.Println("ints failed")
	}

	words := []string{"pear", "apple", "banana", "apple"}
	sort.Strings(words)
	if sort.StringsAreSorted(words) && words[0] == "apple" && words[3] == "pear" {
		fmt.Println("strings sorted")
	} else {
		fmt.Println("strings failed")
	}

	floats := []float64{3.5, -2.25, 0.5}
	sort.Float64s(floats)
	if sort.Float64sAreSorted(floats) && floats[0] == -2.25 && floats[2] == 3.5 {
		fmt.Println("floats sorted")
	} else {
		fmt.Println("floats failed")
	}
}

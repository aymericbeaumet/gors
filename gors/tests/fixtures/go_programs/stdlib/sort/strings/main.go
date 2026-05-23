package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []string{"pear", "apple", "banana"}
	sort.Strings(values)
	if values[0] == "apple" && values[2] == "pear" {
		fmt.Println("sorted")
	} else {
		fmt.Println("failed")
	}
}

package main

import (
	"fmt"
	"sort"
)

func main() {
	values := []string{"apple", "banana", "pear"}
	if sort.StringsAreSorted(values) {
		fmt.Println("sorted")
	} else {
		fmt.Println("failed")
	}
}

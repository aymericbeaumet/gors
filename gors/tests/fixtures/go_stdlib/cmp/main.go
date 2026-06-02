package main

import (
	"cmp"
	"fmt"
)

// gors:stdlib-cover cmp::Ordered
func orderedMin[T cmp.Ordered](a T, b T) T {
	if cmp.Less(b, a) {
		return b
	}
	return a
}

func main() {
	// gors:stdlib-cover cmp::Compare
	// gors:stdlib-cover cmp::Less
	// gors:stdlib-cover cmp::Or
	fmt.Println(cmp.Compare(2, 5), cmp.Compare("go", "rust"), cmp.Less(2.5, 7.5), cmp.Or(0, 5, 7), orderedMin(4, 2))
}

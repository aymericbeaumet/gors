package main

import (
	"cmp"
	"fmt"
)

func main() {
	fmt.Println(cmp.Compare(2, 5), cmp.Compare("go", "rust"), cmp.Less(2.5, 7.5))
}

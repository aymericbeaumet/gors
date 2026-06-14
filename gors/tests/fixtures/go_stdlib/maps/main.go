package main

import (
	"fmt"
	"maps"
)

func main() {
	fmt.Println("== maps/basic ==")
	m := map[string]int{"one": 1, "two": 2}
	fmt.Println(m["two"])
	fmt.Println(maps.Equal(m, map[string]int{"one": 1, "two": 2}))
}

package main

import "fmt"

func main() {
	counts := map[string]int{"seen": 1}
	counts["seen"]++
	counts["missing"]++
	counts["seen"]--
	fmt.Println(counts["seen"], counts["missing"])
}

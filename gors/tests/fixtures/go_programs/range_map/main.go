package main

import "fmt"

func main() {
	values := map[string]int{"a": 1, "b": 2, "c": 3}
	sum := 0
	seen := ""
	for key, value := range values {
		sum += value
		seen += key
	}
	fmt.Println(sum)
	fmt.Println(len(seen))

	keys := 0
	for key := range values {
		keys += len(key)
	}
	fmt.Println(keys)

	lastKey := ""
	lastValue := 0
	for lastKey, lastValue = range values {
	}
	fmt.Println(len(lastKey))
	fmt.Println(lastValue > 0)
}

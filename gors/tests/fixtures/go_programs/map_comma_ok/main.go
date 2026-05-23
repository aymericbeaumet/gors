package main

import "fmt"

func main() {
	m := map[string]int{
		"alice": 25,
		"bob":   30,
	}

	val, ok := m["alice"]
	fmt.Println(val, ok)

	val2, ok2 := m["charlie"]
	fmt.Println(val2, ok2)

	_, exists := m["bob"]
	fmt.Println(exists)
}

package main

import "fmt"

func main() {
	// max with integers
	fmt.Println(max(3, 7))
	fmt.Println(max(10, 2))

	// min with integers
	fmt.Println(min(3, 7))
	fmt.Println(min(10, 2))

	// max with floats
	fmt.Println(max(3.14, 2.71))

	// min with floats
	fmt.Println(min(3.14, 2.71))

	// max with strings
	fmt.Println(max("apple", "banana"))

	// min with strings
	fmt.Println(min("apple", "banana"))
}

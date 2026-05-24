package main

import "fmt"

type Person struct {
	Name string
	City string
}

func main() {
	p := Person{Name: "Alice", City: "Paris"}
	fmt.Println(p.Name)
	fmt.Println(p.City)

	greeting := "Hello"
	fmt.Println(greeting)
}

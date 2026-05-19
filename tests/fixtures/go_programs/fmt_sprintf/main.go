package main

import "fmt"

func main() {
	name := "World"
	s := fmt.Sprintf("Hello, %s!", name)
	fmt.Println(s)

	n := 42
	s2 := fmt.Sprintf("The answer is %d", n)
	fmt.Println(s2)
}

package main

import "fmt"

func main() {
	s := "hello"
	b := []byte(s)
	fmt.Println(len(b))
	fmt.Println(b[0])

	s2 := string(b)
	fmt.Println(s2)

	r := []rune("world")
	fmt.Println(len(r))

	n := string(65)
	fmt.Println(n)
}

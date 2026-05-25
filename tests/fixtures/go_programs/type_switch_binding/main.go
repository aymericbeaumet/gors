package main

import "fmt"

func main() {
	var i interface{}
	_ = i
	fmt.Println("type switch binding works")
}

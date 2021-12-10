package main

import "fmt"

func main() {
	hello("World")
}

func hello(name string) {
	fmt.Print("Hello, ")
	fmt.Print(name)
	fmt.Println("!")
}

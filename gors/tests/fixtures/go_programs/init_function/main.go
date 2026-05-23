package main

import "fmt"

var greeting string

func init() {
	greeting = "hello"
}

func main() {
	fmt.Println(greeting)
}

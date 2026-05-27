package main

import "fmt"

var greeting string = "go"
var count int8 = 40
var suffix string

func main() {
	greeting += "rs"
	suffix = "!"
	count += 2
	fmt.Println(greeting + suffix, count)
}

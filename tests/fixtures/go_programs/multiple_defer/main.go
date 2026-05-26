package main

import "fmt"

func printLabel(s string) {
	fmt.Println("label", s)
}

func printer(prefix string) func(string) {
	return func(s string) {
		fmt.Println(prefix, s)
	}
}

func main() {
	fmt.Println("start")
	msg := "initial"
	defer fmt.Println("saved", msg)
	defer printLabel(msg)
	fn := printer("fn")
	defer fn(msg)
	fn = printer("updated-fn")
	msg = "changed"
	fmt.Println("current", msg)
	defer fmt.Println("first")
	defer fmt.Println("second")
	defer fmt.Println("third")
	fmt.Println("end")
}

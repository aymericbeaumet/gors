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

func printSum(label string, nums ...int) {
	total := 0
	for _, n := range nums {
		total += n
	}
	fmt.Println(label, total)
}

func deferNamedReturn() (out int) {
	defer func() {
		out = 7
	}()
	out = 3
	return
}

func nestedDefer() {
	fmt.Println("nested start")
	if true {
		defer fmt.Println("nested defer")
	}
	fmt.Println("nested end")
}

func main() {
	fmt.Println("start")
	msg := "initial"
	defer fmt.Println("saved", msg)
	defer printLabel(msg)
	fn := printer("fn")
	defer fn(msg)
	fn = printer("updated-fn")
	variadic := printSum
	values := []int{4, 5}
	defer variadic("defer spread", values...)
	defer variadic("defer packed", 1, 2, 3)
	msg = "changed"
	fmt.Println("current", msg)
	defer fmt.Println("first")
	defer fmt.Println("second")
	defer fmt.Println("third")
	fmt.Println("named", deferNamedReturn())
	nestedDefer()
	fmt.Println("end")
}

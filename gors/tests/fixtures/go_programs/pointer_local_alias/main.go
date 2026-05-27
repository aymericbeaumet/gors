package main

import "fmt"

func main() {
	x := 1
	p := &x
	*p = 2
	fmt.Println(x, *p)
	x = 3
	fmt.Println(*p)
}

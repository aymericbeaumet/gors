package main

import "fmt"

type holder struct {
	value int
	other int
}

func main() {
	h := holder{value: 1, other: 2}
	p := &h.value
	*p = 7
	fmt.Println(h.value, h.other)
}

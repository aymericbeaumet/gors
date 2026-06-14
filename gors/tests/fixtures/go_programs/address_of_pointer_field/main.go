package main

import "fmt"

type holder struct {
	value int
	other int
}

func set(h *holder) {
	p := &h.value
	*p = 7
}

func main() {
	h := &holder{value: 1, other: 2}
	set(h)
	fmt.Println(h.value, h.other)
}

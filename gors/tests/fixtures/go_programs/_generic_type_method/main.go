package main

import "fmt"

type Holder[T any] struct {
	value T
}

func (h Holder[T]) Get() T {
	return h.value
}

func (h *Holder[T]) Set(value T) {
	h.value = value
}

func main() {
	ints := Holder[int]{value: 3}
	fmt.Println(ints.Get())
	ints.Set(5)
	fmt.Println(ints.Get())

	strings := Holder[string]{value: "go"}
	fmt.Println(strings.Get())
}

package main

import "fmt"

type Holder[T any] struct {
	Value T
}

func (h Holder[T]) Get() T {
	return h.Value
}

func Identity[T int | string](value T) T {
	return value
}

func Max[T int | float64](a T, b T) T {
	if a > b {
		return a
	}
	return b
}

func PickFirst[T int | string, U int | string](first T, second U) T {
	_ = second
	return first
}

func main() {
	holder := Holder[string]{Value: "value"}
	fmt.Println(holder.Get(), Identity(42), Identity("go"))
	fmt.Println(Max(3, 5), Max(2.5, 1.5))
	fmt.Println(PickFirst("left", 12), PickFirst(7, "right"))
}

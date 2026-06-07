package main

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
	if holder.Get() != "value" || Identity(42) != 42 || Identity("go") != "go" {
		panic("generic identity changed")
	}
	if Max(3, 5) != 5 || Max(2.5, 1.5) != 2.5 {
		panic("generic ordered max changed")
	}
	if PickFirst("left", 12) != "left" || PickFirst(7, "right") != 7 {
		panic("generic multi-parameter inference changed")
	}
}

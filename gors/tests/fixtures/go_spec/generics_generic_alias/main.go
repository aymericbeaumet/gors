package main

type Vector[T any] = []T

type Pair[A, B any] struct {
	First  A
	Second B
}

type IntPair[B any] = Pair[int, B]

func main() {
	var values Vector[int] = []int{1, 2, 3}
	values = append(values, 4)
	if len(values) != 4 || values[3] != 4 {
		panic("generic slice alias changed")
	}
	pair := IntPair[string]{First: 1, Second: "two"}
	same := Pair[int, string]{First: 1, Second: "two"}
	if pair.First != same.First || pair.Second != same.Second {
		panic("generic alias values changed")
	}
	var direct Pair[int, string] = pair
	var aliased IntPair[string] = same
	if direct.Second != "two" || aliased.First != 1 {
		panic("generic alias assignment changed")
	}
	println("generics-generic-alias: ok")
}

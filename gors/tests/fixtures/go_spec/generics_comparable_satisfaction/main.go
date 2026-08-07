package main

func eq[T comparable](left, right T) bool { return left == right }

func acceptsComparable[T comparable]() {}

type Valuer interface{ Value() int }

type V struct{}

func (V) Value() int { return 0 }

type payload struct{ value any }

func main() {
	var left any = 1
	var right any = 1
	if !eq[any](left, right) || eq[any](left, "one") {
		panic("ordinary interface comparable satisfaction changed")
	}
	var first, second Valuer = V{}, V{}
	if !eq[Valuer](first, second) {
		panic("non-basic interface comparable satisfaction changed")
	}
	acceptsComparable[payload]()
	println("generics-comparable-satisfaction: ok")
}

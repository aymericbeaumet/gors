package main

type Producer[T any] interface{ Produce() T }

func produce[P Producer[T], T any](p P) T { return p.Produce() }

type Word struct{}

func (Word) Produce() string { return "word" }

type Num struct{}

func (Num) Produce() int { return 7 }

func main() {
	if produce(Word{}) != "word" || produce(Num{}) != 7 {
		panic("method constraint inference changed")
	}
	println("generics-inference-method-constraints: ok")
}

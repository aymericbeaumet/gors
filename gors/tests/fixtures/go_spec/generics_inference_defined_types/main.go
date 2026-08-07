package main

type Values []int

func (values Values) Tag() string { return "Values" }

func keep[S ~[]E, E any](values S) S { return values }

func choose[T any](first, second T) T { return second }

func main() {
	kept := keep(Values{1, 2})
	first := choose(Values{9}, []int{4, 5})
	second := choose([]int{4, 5}, Values{9})
	if kept.Tag() != "Values" || first.Tag() != "Values" || second.Tag() != "Values" {
		panic("defined type inference changed")
	}
	if first[0] != 4 || second[0] != 9 {
		panic("defined type result changed")
	}
	println("generics-inference-defined-types: ok")
}

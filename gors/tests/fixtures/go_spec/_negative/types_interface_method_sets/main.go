package main

type PointerOnly interface {
	Mutate()
}

type Value struct{}

func (v *Value) Mutate() {}

func main() {
	var _ PointerOnly = Value{}
}

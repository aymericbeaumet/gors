package main

type Float interface {
	~float32 | ~float64
}

func main() {
	// illegal: Float is not a basic interface; interfaces that are not basic
	// may only be used as type constraints.
	var x Float
	_ = x
}

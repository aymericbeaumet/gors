package main

func failedConcreteAssertionPanics() (recovered interface{}) {
	defer func() { recovered = recover() }()
	var i interface{} = 7
	_ = i.(string)
	return nil
}

func main() {
	r := failedConcreteAssertionPanics()
	if r == nil {
		panic("failed concrete type assertion did not panic")
	}
	if _, ok := r.(error); !ok {
		panic("type assertion run-time panic value does not satisfy error")
	}
	var i interface{} = 7
	s, ok := i.(string)
	if ok || s != "" {
		panic("comma-ok assertion misbehaved")
	}
	println("run-time-panics-type-assertion-concrete: ok")
}

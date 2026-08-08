package main

func recoverFromNilPanic() (recovered any, isError bool) {
	defer func() {
		r := recover()
		recovered = r
		_, isError = r.(error)
	}()
	panic(nil)
}

func main() {
	recovered, isError := recoverFromNilPanic()
	if recovered == nil {
		panic("recover returned nil after panic(nil)")
	}
	if !isError {
		panic("panic(nil) run-time panic value does not satisfy error")
	}

	var p *int
	defer func() {
		r := recover()
		if r == nil {
			panic("recover returned nil interface for typed nil pointer panic")
		}
		q, ok := r.(*int)
		if !ok || q != nil {
			panic("typed nil pointer panic value changed")
		}
		println("panic-nil-runtime-panic: ok")
	}()
	panic(p)
}

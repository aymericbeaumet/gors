package main

func recoverNilPanic() (recovered interface{}, isError bool) {
	defer func() {
		recovered = recover()
		_, isError = recovered.(error)
	}()
	panic(nil)
}

func recoverTypedNilPanic() (recovered interface{}, isError bool) {
	defer func() {
		recovered = recover()
		_, isError = recovered.(error)
	}()
	var e error
	panic(e)
}

func main() {
	r, isErr := recoverNilPanic()
	if r == nil {
		panic("recover returned nil after panic(nil)")
	}
	if !isErr {
		panic("run-time panic value for panic(nil) does not satisfy error")
	}

	r, isErr = recoverTypedNilPanic()
	if r == nil {
		panic("recover returned nil after panic(nil error interface)")
	}
	if !isErr {
		panic("run-time panic value for panic(nil interface) does not satisfy error")
	}

	println("handling-panics-nil-panic: ok")
}

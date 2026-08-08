package main

type failure struct{}

func (*failure) Error() string { return "failed" }

func maybeFail(fail bool) error {
	var f *failure
	allocated := &failure{}
	if fail {
		f = allocated
	}
	return f
}

func main() {
	var none error
	if none != nil {
		panic("zero value of error is not nil")
	}
	err := maybeFail(false)
	if err == nil {
		panic("error interface holding a nil *failure compared equal to nil")
	}
	failed := maybeFail(true)
	if failed == nil || failed.Error() != "failed" {
		panic("real error changed")
	}
	println("typed-nil error is non-nil:", err != nil)
}

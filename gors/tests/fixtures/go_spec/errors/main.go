package main

type valueError struct{}

func (valueError) Error() string {
	return "value error"
}

type pointerError struct{}

func (*pointerError) Error() string {
	return "pointer error"
}

func choose(flag bool) error {
	if flag {
		return valueError{}
	}
	return &pointerError{}
}

func main() {
	var err error
	if err != nil {
		panic("zero error is not nil")
	}

	err = valueError{}
	if err == nil || err.Error() != "value error" {
		panic("value error changed")
	}

	err = &pointerError{}
	if err == nil || err.Error() != "pointer error" {
		panic("pointer error changed")
	}

	a := choose(true)
	b := choose(false)
	if a == nil || a.Error() != "value error" {
		panic("chosen value error changed")
	}
	if b == nil || b.Error() != "pointer error" {
		panic("chosen pointer error changed")
	}
}

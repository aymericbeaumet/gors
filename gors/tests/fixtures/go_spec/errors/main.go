package main

import "fmt"

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
	fmt.Println(err == nil)

	err = valueError{}
	fmt.Println(err.Error())
	fmt.Println(err != nil)

	err = &pointerError{}
	fmt.Println(err.Error())

	a := choose(true)
	b := choose(false)
	fmt.Println(a.Error())
	fmt.Println(b.Error())
}

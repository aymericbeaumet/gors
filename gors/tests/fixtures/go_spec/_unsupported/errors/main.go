package main

import "fmt"

type myError struct {
	msg string
}

func (e *myError) Error() string {
	return e.msg
}

func main() {
	var err error
	fmt.Println(err == nil)

	err = &myError{"test error"}
	fmt.Println(err.Error())
	fmt.Println(err != nil)
}

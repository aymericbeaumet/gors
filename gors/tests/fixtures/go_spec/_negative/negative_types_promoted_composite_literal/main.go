package main

import "fmt"

type Base struct{ Name string }

type Wrap struct {
	Base
}

func main() {
	// illegal: promoted fields "cannot be used as field names in composite
	// literals of the struct".
	w := Wrap{Name: "x"}
	fmt.Println(w)
}

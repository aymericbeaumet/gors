package main

import "fmt"

func find(ok bool) *int {
	if ok {
		x := 42
		return &x
	}
	return nil
}

func main() {
	if find(true) != nil {
		fmt.Println("found")
	}
	if find(false) == nil {
		fmt.Println("missing")
	}
}

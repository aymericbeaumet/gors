package main

import "fmt"

func safe() {
	defer func() {
		if recover() != nil {
			fmt.Println("recovered")
		}
	}()
	panic("boom")
	fmt.Println("unreachable")
}

func main() {
	safe()
	fmt.Println("after")
}

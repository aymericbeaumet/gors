package main

import "fmt"

func main() {
	print := func() {
		fmt.Println("local print")
	}
	print()

	println := func() {
		fmt.Println("local println")
	}
	println()

	prints := []func(){
		func() {
			fmt.Println("range print 1")
		},
		func() {
			fmt.Println("range print 2")
		},
	}
	for _, print := range prints {
		print()
	}
}

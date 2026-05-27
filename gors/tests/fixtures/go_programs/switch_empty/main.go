package main

import "fmt"

func tag() int {
	fmt.Println("tag")
	return 1
}

func main() {
	fmt.Println("before")
	switch tag() {
	}
	switch {
	}
	fmt.Println("after")
}

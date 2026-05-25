package main

import "fmt"

func main() {
	fmt.Println("start")
	defer fmt.Println("first")
	defer fmt.Println("second")
	defer fmt.Println("third")
	fmt.Println("end")
}

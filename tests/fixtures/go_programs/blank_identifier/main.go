package main

import "fmt"

func twoValues() (int, int) {
	return 10, 20
}

func main() {
	_, b := twoValues()
	fmt.Println(b)
	a, _ := twoValues()
	fmt.Println(a)
}

package main

import "fmt"

func triggerOutOfBounds(s []int, idx int) {
	_ = s[idx]
}

func testOutOfBounds() {
	defer func() {
		if r := recover(); r != nil {
			fmt.Println("recovered out of bounds")
		}
	}()
	s := []int{1}
	triggerOutOfBounds(s, 2)
}

func triggerNilPointer(p *int) {
	_ = *p
}

func testNilPointer() {
	defer func() {
		if r := recover(); r != nil {
			fmt.Println("recovered nil pointer")
		}
	}()
	triggerNilPointer(nil)
}

func triggerDivideByZero(a, b int) {
	_ = a / b
}

func testDivideByZero() {
	defer func() {
		if r := recover(); r != nil {
			fmt.Println("recovered divide by zero")
		}
	}()
	triggerDivideByZero(1, 0)
}

func main() {
	testOutOfBounds()
	testNilPointer()
	testDivideByZero()
}

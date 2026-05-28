package main

import "fmt"

func pair() (int, int) {
	return 3, 4
}

func returnCall() (int, int) {
	return pair()
}

func namedReturn() (left int, right int) {
	left = 5
	right = 6
	return
}

func deferredNamedReturn() (result int) {
	defer func() {
		result += 2
	}()
	return 7
}

func main() {
	callLeft, callRight := returnCall()
	namedLeft, namedRight := namedReturn()
	fmt.Println(callLeft, callRight, namedLeft, namedRight, deferredNamedReturn())
}

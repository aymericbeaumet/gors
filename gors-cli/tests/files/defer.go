package main

import "fmt"

func main() {
	lock(l)
	defer unlock(l) // unlocking happens before surrounding function returns

	// prints 3 2 1 0 before surrounding function returns
	for i := 0; i <= 3; i++ {
		defer fmt.Print(i)
	}
}

// f returns 42
func f() (result int) {
	defer func() {
		// result is accessed after it was set to 6 by the return statement
		result *= 7
	}()
	return 6
}

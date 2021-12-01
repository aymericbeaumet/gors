package main

import "fmt"

func main() {
	for i := 1; i < 100; i++ {
		fizzbuzz(i)
	}
}

func fizzbuzz(i int) {
	if i%15 == 0 {
		fmt.Println("Fizz Buzz")
	} else if i%3 == 0 {
		fmt.Println("Fizz")
	} else if i%5 == 0 {
		fmt.Println("Buzz")
	} else {
		fmt.Println(i)
	}
}

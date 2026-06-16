package main

import "fmt"

func main() {
	ch := make(chan string, 2)
	ch <- "first"
	ch <- "second"
	switch any(<-ch).(type) {
	}
	fmt.Println(<-ch)

	ch2 := make(chan string, 4)
	ch2 <- "first"
	ch2 <- "second"
	ch2 <- "third"
	ch2 <- "fourth"
	switch any(<-ch2).(type) {
	case int:
		fmt.Println("int")
	case bool:
		fmt.Println("bool")
	case float64:
		fmt.Println("float")
	default:
		fmt.Println(<-ch2)
	}
}

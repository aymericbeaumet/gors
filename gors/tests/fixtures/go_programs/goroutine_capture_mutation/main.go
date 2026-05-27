package main

import "fmt"

func main() {
	done := make(chan bool)
	count := 0

	go func() {
		count = 7
		done <- true
	}()

	<-done
	fmt.Println(count)
}

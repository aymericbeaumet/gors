package main

import "fmt"

func main() {
	err := fmt.Errorf("value %d failed", 7)
	fmt.Println(err)
}

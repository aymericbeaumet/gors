package main

import "fmt"

func main() {
	i := 0
Loop:
	if i < 4 {
		fmt.Println(i)
		i++
		goto Loop
	}
}

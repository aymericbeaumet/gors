package main

import "fmt"

func main() {
	x := 1
	goto Done
	fmt.Println("skipped")
Done:
	fmt.Println(x)
}

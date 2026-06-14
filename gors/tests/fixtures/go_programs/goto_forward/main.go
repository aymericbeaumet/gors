package main

import "fmt"

func main() {
	fmt.Println("start")
	if true {
		goto Done
	}
	fmt.Println("skipped")
Done:
	fmt.Println("done")
}

package main

import "fmt"

func main() {
	total := 0
	if true {
		total++
		goto Done
	}
	total = 100
Done:
	total += 2
	switch total {
	case 3:
		goto Print
	default:
		total = 100
	}
Print:
	fmt.Println(total)
}

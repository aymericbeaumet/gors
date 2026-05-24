package main

import "fmt"

func main() {
	out := fmt.Sprintf("%s=%d %c %.2f", "value", 7, 65, 3.5)
	fmt.Println(out)
}

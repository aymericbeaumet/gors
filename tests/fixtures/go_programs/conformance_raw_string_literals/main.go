package main

import "fmt"

func main() {
	lines := `first
second`
	escapes := `\n\t`
	fmt.Println(len(lines), lines == "first\nsecond")
	fmt.Println(escapes)
}

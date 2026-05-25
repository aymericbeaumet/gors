package main

import (
	"fmt"
	"os"
)

func main() {
	fmt.Fprintf(os.Stdout, "Hello, %s!\n", "World")
	fmt.Fprintf(os.Stdout, "Number: %d\n", 42)
}

package main

import (
	"fmt"
	"os"
)

func main() {
	fmt.Fprint(os.Stdout, "value", 7)
	fmt.Println("")
}

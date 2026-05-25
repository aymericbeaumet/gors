package main

import (
	"errors"
	"fmt"
)

func main() {
	err := errors.New("boom")
	fmt.Println(err)
}

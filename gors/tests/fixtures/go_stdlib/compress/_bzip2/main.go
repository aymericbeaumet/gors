package main

import (
	"compress/bzip2"
	"fmt"
)

func caseBzip2StructuralError() {
	// gors:stdlib-cover compress/bzip2::StructuralError
	msg := "bad block"
	err := bzip2.StructuralError(msg)
	// gors:stdlib-cover compress/bzip2::StructuralError.Error
	fmt.Println(err.Error())
}

func main() {
	caseBzip2StructuralError()
}

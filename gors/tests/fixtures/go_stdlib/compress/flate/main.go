package main

import (
	"compress/flate"
	"fmt"
)

type errString string

func (e errString) Error() string {
	return string(e)
}

func caseFlateConstants() {
	// gors:stdlib-cover compress/flate::BestCompression
	// gors:stdlib-cover compress/flate::BestSpeed
	// gors:stdlib-cover compress/flate::DefaultCompression
	// gors:stdlib-cover compress/flate::HuffmanOnly
	// gors:stdlib-cover compress/flate::NoCompression
	fmt.Println(flate.NoCompression, flate.BestSpeed, flate.BestCompression, flate.DefaultCompression, flate.HuffmanOnly)
}

func caseFlateErrors() {
	// gors:stdlib-cover compress/flate::CorruptInputError
	corrupt := flate.CorruptInputError(12)
	// gors:stdlib-cover compress/flate::CorruptInputError.Error
	fmt.Println(corrupt.Error())

	// gors:stdlib-cover compress/flate::InternalError
	internalMessage := "bad code"
	internal := flate.InternalError(internalMessage)
	// gors:stdlib-cover compress/flate::InternalError.Error
	fmt.Println(internal.Error())

	// gors:stdlib-cover compress/flate::ReadError
	readErr := &flate.ReadError{Offset: 3, Err: errString("short read")}
	// gors:stdlib-cover compress/flate::ReadError.Error
	fmt.Println(readErr.Error())

	// gors:stdlib-cover compress/flate::WriteError
	writeErr := &flate.WriteError{Offset: 5, Err: errString("short write")}
	// gors:stdlib-cover compress/flate::WriteError.Error
	fmt.Println(writeErr.Error())
}

func main() {
	caseFlateConstants()
	caseFlateErrors()
}

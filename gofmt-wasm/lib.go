package main

import (
	gofmt "go/format"
	"syscall/js"
)

func main() {
	c := make(chan struct{}, 0)
	js.Global().Set("format", js.FuncOf(format))
	<-c
}

func format(this js.Value, p []js.Value) interface{} {
	result, err := gofmt.Source([]byte(p[0].String()))
	if err != nil {
		panic(err)
	}
	return js.ValueOf(string(result))
}

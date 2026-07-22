package main

import (
	"example/contracts"
	"example/values"
)

type Narrow interface {
	Name() string
}

type Wide interface {
	Name() string
	Detail() string
}

type detailed struct{}

func (detailed) Name() string {
	return "name"
}

func (detailed) Detail() string {
	return "detail"
}

type wrapper struct {
	Narrow
}

func main() {
	reader := values.AsReader(values.NewStore(1))
	writer, ok := reader.(contracts.Writer)
	if !ok {
		panic("defined map lost its second interface")
	}
	writer.Write(7)
	if reader.Read() != 7 {
		panic("defined map interface assertion lost map identity")
	}
	if !contracts.HasAnonymousWriter(reader) {
		panic("opaque external value lost anonymous interface assertion")
	}

	var narrow Narrow = wrapper{Narrow: detailed{}}
	if narrow.Name() != "name" {
		panic("embedded base method was not promoted")
	}
	if _, ok := narrow.(Wide); ok {
		panic("embedded wrapper exposed its field's dynamic type")
	}

	println("ok")
}

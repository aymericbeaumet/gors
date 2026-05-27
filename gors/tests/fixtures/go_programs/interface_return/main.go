package main

import "fmt"

type Greeter interface {
	Greet() string
}

type Person struct {
	Name string
}

func (p Person) Greet() string {
	return "hello " + p.Name
}

func MakeGreeter(name string) Greeter {
	return Person{Name: name}
}

func main() {
	g := MakeGreeter("gors")
	fmt.Println(g.Greet())
}

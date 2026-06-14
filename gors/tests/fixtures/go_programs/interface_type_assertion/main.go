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

func main() {
	var x any = Person{Name: "gors"}
	g, ok := x.(Greeter)
	fmt.Println(ok)
	fmt.Println(g.Greet())
}

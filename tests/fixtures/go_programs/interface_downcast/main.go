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
	var g Greeter = Person{Name: "gors"}
	p, ok := g.(Person)
	fmt.Println(ok)
	fmt.Println(p.Name)
}

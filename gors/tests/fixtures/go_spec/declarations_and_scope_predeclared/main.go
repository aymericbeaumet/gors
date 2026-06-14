package main

func main() {
	len := 42
	if len != 42 {
		panic("predeclared name shadowing failed")
	}
}

package main

const message = "héllo"

func main() {
	var buffer [len(message)]byte
	const combined = len("go" + "rs")
	var pair [combined - 2]int
	var empty [len("")]struct{}
	if len(buffer) != 6 {
		panic("constant string length is not the byte length")
	}
	if combined != 4 || len(pair) != 2 || len(empty) != 0 {
		panic("constant len arithmetic changed")
	}
	println(len(buffer), combined, len(pair), len(empty))
}

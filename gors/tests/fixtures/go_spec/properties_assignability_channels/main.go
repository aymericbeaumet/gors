package main



type Duplex chan int

func main() {
	// Unnamed bidirectional channel assigns to the named type Duplex:
	// identical underlying types and the source type is not named.
	var d Duplex = make(chan int, 2)

	// Named bidirectional channel assigns to unnamed directional channel
	// types: identical element type, V is bidirectional, T is not named.
	var send chan<- int = d
	var recv <-chan int = d

	send <- 41
	send <- 1
	first := <-recv
	second := <-d
	println(first+second, cap(d), len(d))

	// The same rules apply for function arguments (assignability governs calls).
	report := func(out chan<- int, in <-chan int) int {
		out <- 7
		return <-in
	}
	println(report(d, d))
}
